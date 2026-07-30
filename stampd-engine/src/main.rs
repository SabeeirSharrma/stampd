use std::path::PathBuf;
use tracing::{error, info};

// Use the engine library for all modules
use stampd_engine::{
    api, config::Config, db, dkim, maildir, queue, smtpd, stats, submissiond, tls, ENGINE_DB,
    ENGINE_STATS,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load config
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("stampd.toml"));

    let config = Config::load(&config_path)?;
    info!(?config, "Loaded configuration");

    // Ensure parent directories for DB and filters exist
    if let Some(parent) = std::path::Path::new(&config.engine.db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&config.engine.filters_dir)?;

    // Initialize database
    let database = Arc::new(db::Database::open(std::path::Path::new(
        &config.engine.db_path,
    ))?);
    info!(path = %config.engine.db_path, "Database initialized");

    // Set global database handle for napi exports
    let _ = ENGINE_DB.set(database.clone());

    // Initialize engine stats
    let engine_stats = stats::EngineStats::new();
    let _ = ENGINE_STATS.set(engine_stats.clone());

    // Seed server_config if it doesn't exist
    match database.get_server_config() {
        Ok(_) => info!("Server config exists"),
        Err(_) => {
            let conn = {
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

    // Load TLS config for STARTTLS
    let tls_config = tls::try_load(
        config
            .engine
            .tls_cert_path
            .as_ref()
            .map(std::path::Path::new),
        config
            .engine
            .tls_key_path
            .as_ref()
            .map(std::path::Path::new),
    );

    // Initialize DKIM signer
    let dkim_signer = match dkim::DkimSigner::new(
        &config.engine.domain,
        &config.engine.dkim_selector,
        std::path::Path::new(&config.engine.dkim_key_dir),
    ) {
        Ok(signer) => {
            info!(selector = %config.engine.dkim_selector, "DKIM signer initialized");
            Some(signer)
        }
        Err(e) => {
            error!(error = ?e, "Failed to initialize DKIM signer — outgoing mail unsigned");
            None
        }
    };

    // Start inbound SMTP server (with TLS and filters)
    let smtp_tls = tls_config.clone();
    let smtp_stats = engine_stats.clone();
    let smtp_handle = tokio::spawn(smtpd::run(
        config.engine.smtp_port,
        config.engine.maildir_path.clone(),
        config.engine.domain.clone(),
        database.clone(),
        smtp_tls,
        std::path::PathBuf::from(&config.engine.filters_dir),
        config.engine.filters_timeout_ms,
        config.engine.gateway_url.clone(),
        smtp_stats,
    ));

    // Start outbound submission server (with DKIM and TLS)
    let submission_dkim = dkim_signer.clone();
    let submission_tls = tls_config.as_ref().map(|tc| tc.server_config.clone());
    let submission_stats = engine_stats.clone();
    let submission_handle = tokio::spawn(submissiond::run(
        config.engine.submission_port,
        config.engine.dkim_selector.clone(),
        database.clone(),
        submission_dkim,
        submission_tls,
        submission_stats,
    ));

    // Start queue processor
    let queue_handle = tokio::spawn(queue::run(
        database.clone(),
        config.engine.maildir_path.clone(),
    ));

    // Start internal API server
    let api_stats = engine_stats.clone();
    let api_handle = tokio::spawn(api::run(
        config.engine.api_port,
        database.clone(),
        api_stats,
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
        result = api_handle => {
            match result {
                Ok(Ok(())) => info!("Internal API stopped"),
                Ok(Err(e)) => error!("Internal API error: {:?}", e),
                Err(e) => error!("Internal API panic: {:?}", e),
            }
        }
    }

    Ok(())
}
