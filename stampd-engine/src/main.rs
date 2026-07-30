use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

// Use the engine library for all modules
use stampd_engine::{
    api, config::Config, db, dkim, maildir, queue, smtpd, stats, submissiond, tls, ENGINE_DB,
    ENGINE_STATS,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Check for --version flag
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("stampd-engine v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

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

    let config = Arc::new(RwLock::new(config));
    let config_for_reload = config.clone();
    let config_path_for_reload = config_path.clone();

    // Ensure parent directories for DB and filters exist
    {
        let cfg = config.read().await;
        if let Some(parent) = std::path::Path::new(&cfg.engine.db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(&cfg.engine.filters_dir)?;
    }

    // Initialize database
    let db_path = {
        let cfg = config.read().await;
        cfg.engine.db_path.clone()
    };
    let database = Arc::new(db::Database::open(std::path::Path::new(&db_path))?);
    info!(path = %db_path, "Database initialized");

    // Set global database handle for napi exports
    let _ = ENGINE_DB.set(database.clone());

    // Initialize engine stats
    let engine_stats = stats::EngineStats::new();
    let _ = ENGINE_STATS.set(engine_stats.clone());

    // Seed server_config if it doesn't exist
    let (domain, dkim_selector) = {
        let cfg = config.read().await;
        (cfg.engine.domain.clone(), cfg.engine.dkim_selector.clone())
    };
    match database.get_server_config() {
        Ok(_) => info!("Server config exists"),
        Err(_) => {
            let conn = {
                let c = rusqlite::Connection::open(&db_path)?;
                c.execute(
                    "INSERT INTO server_config (id, domain, signup_enabled, dkim_selector) VALUES (1, ?1, 1, ?2)",
                    rusqlite::params![domain, dkim_selector],
                )?;
                info!(domain = %domain, "Seeded server_config");
                c
            };
            drop(conn);
        }
    }

    // Initialize maildir
    {
        let cfg = config.read().await;
        maildir::init(&cfg.engine.maildir_path).await?;
    }

    // Load TLS config for STARTTLS
    let tls_config = {
        let cfg = config.read().await;
        tls::try_load(
            cfg.engine.tls_cert_path.as_ref().map(std::path::Path::new),
            cfg.engine.tls_key_path.as_ref().map(std::path::Path::new),
        )
    };

    // Initialize DKIM signer
    let dkim_signer = {
        let cfg = config.read().await;
        match dkim::DkimSigner::new(
            &cfg.engine.domain,
            &cfg.engine.dkim_selector,
            std::path::Path::new(&cfg.engine.dkim_key_dir),
        ) {
            Ok(signer) => {
                info!(selector = %cfg.engine.dkim_selector, "DKIM signer initialized");
                Some(signer)
            }
            Err(e) => {
                error!(error = ?e, "Failed to initialize DKIM signer — outgoing mail unsigned");
                None
            }
        }
    };

    // Start inbound SMTP server (with TLS and filters)
    let smtp_tls = tls_config.clone();
    let smtp_stats = engine_stats.clone();
    let smtp_config = config.clone();
    let smtp_db = database.clone();
    let smtp_handle = tokio::spawn(async move {
        let cfg = smtp_config.read().await;
        smtpd::run(
            cfg.engine.smtp_port,
            cfg.engine.maildir_path.clone(),
            cfg.engine.domain.clone(),
            smtp_db,
            smtp_tls,
            std::path::PathBuf::from(&cfg.engine.filters_dir),
            cfg.engine.filters_timeout_ms,
            cfg.engine.gateway_url.clone(),
            smtp_stats,
        )
        .await
    });

    // Start outbound submission server (with DKIM and TLS)
    let submission_dkim = dkim_signer.clone();
    let submission_tls = tls_config.as_ref().map(|tc| tc.server_config.clone());
    let submission_stats = engine_stats.clone();
    let submission_config = config.clone();
    let submission_db = database.clone();
    let submission_handle = tokio::spawn(async move {
        let cfg = submission_config.read().await;
        submissiond::run(
            cfg.engine.submission_port,
            cfg.engine.dkim_selector.clone(),
            submission_db,
            submission_dkim,
            submission_tls,
            submission_stats,
        )
        .await
    });

    // Start queue processor
    let queue_config = config.clone();
    let queue_db = database.clone();
    let queue_handle = tokio::spawn(async move {
        let cfg = queue_config.read().await;
        queue::run(queue_db, cfg.engine.maildir_path.clone()).await
    });

    // Start internal API server
    let api_stats = engine_stats.clone();
    let api_db = database.clone();
    let api_handle = tokio::spawn(api::run(
        {
            let cfg = config.read().await;
            cfg.engine.api_port
        },
        api_db,
        api_stats,
        config.clone(),
        config_path.clone(),
    ));

    // SIGHUP handler — reload config (Unix only)
    #[cfg(unix)]
    {
        let sighup_config = config_for_reload;
        let sighup_path = config_path_for_reload;
        tokio::spawn(async move {
            loop {
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
                    .expect("Failed to register SIGHUP handler")
                    .recv()
                    .await;
                info!("Received SIGHUP, reloading configuration...");
                match Config::load(&sighup_path) {
                    Ok(new_config) => {
                        let mut cfg = sighup_config.write().await;
                        *cfg = new_config;
                        info!("Configuration reloaded via SIGHUP");
                    }
                    Err(e) => {
                        error!("Failed to reload config via SIGHUP: {}", e);
                    }
                }
            }
        });
    }

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
