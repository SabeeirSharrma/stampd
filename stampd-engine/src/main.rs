use std::path::PathBuf;
use tracing::{info, error};

mod config;
mod smtpd;
mod submissiond;
mod maildir;
mod queue;
mod db;
mod delivery;

use config::Config;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load config
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("stampd.toml"));

    let config = Config::load(&config_path)?;
    info!(?config, "Loaded configuration");

    // Ensure parent directory for DB exists
    if let Some(parent) = std::path::Path::new(&config.engine.db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Initialize database
    let database = Arc::new(db::Database::open(std::path::Path::new(&config.engine.db_path))?);
    info!(path = %config.engine.db_path, "Database initialized");

    // Seed server_config if it doesn't exist
    match database.get_server_config() {
        Ok(_) => info!("Server config exists"),
        Err(_) => {
            // First run — seed with the domain from stampd.toml
            let conn = {
                // We need to insert via the mutex; open a temporary connection
                let c = rusqlite::Connection::open(&config.engine.db_path)?;
                c.execute(
                    "INSERT INTO server_config (id, domain, signup_enabled, dkim_selector) VALUES (1, ?1, 1, ?2)",
                    rusqlite::params![config.engine.domain, config.engine.dkim_selector],
                )?;
                info!(domain = %config.engine.domain, "Seeded server_config");
                c
            };
            drop(conn);
        }
    }

    // Initialize maildir
    maildir::init(&config.engine.maildir_path).await?;

    // Start inbound SMTP server
    let smtp_handle = tokio::spawn(smtpd::run(
        config.engine.smtp_port,
        config.engine.maildir_path.clone(),
        config.engine.domain.clone(),
        database.clone(),
    ));

    // Start outbound submission server
    let submission_handle = tokio::spawn(submissiond::run(
        config.engine.submission_port,
        config.engine.dkim_selector.clone(),
        database.clone(),
    ));

    // Start queue processor
    let queue_handle = tokio::spawn(queue::run(
        database.clone(),
        config.engine.maildir_path.clone(),
    ));

    info!("Stampd engine started");

    // Wait for any task to complete (or fail)
    tokio::select! {
        result = smtp_handle => {
            match result {
                Ok(Ok(())) => info!("SMTP server stopped"),
                Ok(Err(e)) => error!("SMTP server error: {:?}", e),
                Err(e) => error!("SMTP server panic: {:?}", e),
            }
        }
        result = submission_handle => {
            match result {
                Ok(Ok(())) => info!("Submission server stopped"),
                Ok(Err(e)) => error!("Submission server error: {:?}", e),
                Err(e) => error!("Submission server panic: {:?}", e),
            }
        }
        result = queue_handle => {
            match result {
                Ok(Ok(())) => info!("Queue processor stopped"),
                Ok(Err(e)) => error!("Queue processor error: {:?}", e),
                Err(e) => error!("Queue processor panic: {:?}", e),
            }
        }
    }

    Ok(())
}
