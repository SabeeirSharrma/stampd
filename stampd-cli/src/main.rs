mod config;
mod supervisor;

use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing with service-aware formatting
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("up") => {
            let only: Option<Vec<String>> = args
                .get(2)
                .filter(|s| s.as_str() == "--only")
                .and_then(|_| args.get(3))
                .map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

            cmd_up(only).await?;
        }
        Some("down") => {
            cmd_down().await?;
        }
        Some("restart") => {
            cmd_down().await?;
            cmd_up(None).await?;
        }
        Some("status") => {
            cmd_status().await?;
        }
        Some("logs") => {
            let service = args.get(2).map(|s| s.as_str());
            cmd_logs(service).await?;
        }
        Some("init") => {
            cmd_init().await?;
        }
        _ => {
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("stampd — self-hosted mail server");
    println!();
    println!("Usage:");
    println!("  stampd up                         Start all enabled services");
    println!("  stampd up --only engine,gateway   Start specific services");
    println!("  stampd down                       Stop all services");
    println!("  stampd restart                    Restart all services");
    println!("  stampd status                     Show running services");
    println!("  stampd logs [service]             Show service logs");
    println!("  stampd init                       Initialize configuration");
    println!();
    println!("Services: engine, gateway, admin, web");
    println!();
    println!("Examples:");
    println!("  stampd up                         # Start everything");
    println!("  stampd up --only engine,gateway   # Start only engine + gateway");
    println!("  stampd down                       # Stop everything");
    println!("  stampd status                     # Check what's running");
    println!("  stampd logs gateway               # Watch gateway logs");
}

async fn cmd_up(only: Option<Vec<String>>) -> Result<()> {
    let config = config::load_config("stampd.toml")?;
    let services = config.build_service_list(only);

    if services.is_empty() {
        println!("No services to start. Check stampd.toml configuration.");
        return Ok(());
    }

    info!(
        services = ?services.iter().map(|s| &s.name).collect::<Vec<_>>(),
        "Starting services"
    );

    let pid_dir = std::path::PathBuf::from("/var/run/stampd");
    let log_dir = std::path::PathBuf::from("/var/log/stampd");

    let mut sup = supervisor::Supervisor::new(pid_dir, log_dir);

    for service in &services {
        if !service.enabled {
            info!(service = %service.name, "Skipping disabled service");
            continue;
        }
        sup.add_service(service.clone());
    }

    // Run supervisor until Ctrl+C
    sup.run().await?;

    Ok(())
}

async fn cmd_down() -> Result<()> {
    let config = config::load_config("stampd.toml")?;
    let services = config.build_service_list(None);

    let pid_dir = std::path::PathBuf::from("/var/run/stampd");
    let log_dir = std::path::PathBuf::from("/var/log/stampd");

    let mut sup = supervisor::Supervisor::new(pid_dir, log_dir);
    for service in &services {
        sup.add_service(service.clone());
    }

    println!("Stopping all services...");
    sup.stop_all().await?;
    println!("All services stopped.");

    Ok(())
}

async fn cmd_status() -> Result<()> {
    let config = config::load_config("stampd.toml")?;
    let services = config.build_service_list(None);

    let pid_dir = std::path::PathBuf::from("/var/run/stampd");
    let log_dir = std::path::PathBuf::from("/var/log/stampd");

    let sup = supervisor::Supervisor::new(pid_dir, log_dir);
    let mut sup_with_services = sup;
    for service in &services {
        sup_with_services.add_service(service.clone());
    }

    println!("Stampd Service Status");
    println!("=====================");
    println!();

    for (name, status) in sup_with_services.check_status() {
        let indicator = match status {
            supervisor::ServiceStatus::Running(pid) => format!("✓ running (pid {})", pid),
            supervisor::ServiceStatus::Stopped => "✗ stopped".to_string(),
            supervisor::ServiceStatus::Unknown => "? unknown".to_string(),
        };
        println!("  {:<12} {}", name, indicator);
    }

    println!();
    Ok(())
}

async fn cmd_logs(service: Option<&str>) -> Result<()> {
    let log_dir = std::path::PathBuf::from("/var/log/stampd");

    match service {
        Some(svc) => {
            let log_file = log_dir.join(format!("{}.stderr.log", svc));
            if log_file.exists() {
                println!("=== {} logs (tail) ===", svc);
                // Use tail -f to follow logs
                let output = std::process::Command::new("tail")
                    .args(["-n", "50", "-f", &log_file.to_string_lossy()])
                    .status();
                match output {
                    Ok(_) => {}
                    Err(e) => println!("Failed to tail logs: {}", e),
                }
            } else {
                println!("No logs found for service: {}", svc);
            }
        }
        None => {
            println!("Available log files:");
            if log_dir.exists() {
                for entry in std::fs::read_dir(&log_dir)? {
                    let entry = entry?;
                    if entry.path().extension().is_some_and(|e| e == "log") {
                        println!("  {}", entry.path().display());
                    }
                }
            } else {
                println!("  No log directory found");
            }
            println!();
            println!("Usage: stampd logs <service>");
        }
    }

    Ok(())
}

async fn cmd_init() -> Result<()> {
    let config_path = std::path::Path::new("stampd.toml");
    if config_path.exists() {
        println!("stampd.toml already exists. Delete it first to re-initialize.");
        return Ok(());
    }

    // Create default config
    let default_config = r#"# Stampd Configuration
# See https://sabeeir.qd.je/stampd for full documentation

[engine]
smtp_port = 25
submission_port = 587
maildir_path = "/var/lib/stampd/mail"
db_path = "/var/lib/stampd/stampd.db"
dkim_selector = "default"
api_port = 8090

# TLS for STARTTLS (optional — generates self-signed cert if not set)
# tls_cert_path = "/etc/stampd/tls/cert.pem"
# tls_key_path = "/etc/stampd/tls/key.pem"

# DKIM key storage directory
dkim_key_dir = "/var/lib/stampd/dkim"

# Filter scripts directory
filters_dir = "/var/lib/stampd/filters"
filters_timeout_ms = 500

# Gateway URL for Transit filter delegation (optional)
# When set, the engine delegates filter execution to the gateway's
# Transit Python bridge instead of spawning scripts directly.
gateway_url = "http://127.0.0.1:8080"

[gateway]
enabled = true
port = 8080
rate_limit_per_min = 60
cors_origins = ["*"]

[admin]
enabled = true
port = 8081
signup_enabled = true
default_quota_mb = 5120

[web]
enabled = true
port = 3000

[filters]
enabled = true
timeout_ms = 500
"#;

    std::fs::write(config_path, default_config)?;
    println!("Created stampd.toml with default configuration.");
    println!();
    println!("Next steps:");
    println!("  1. Edit stampd.toml for your domain");
    println!("  2. Run 'stampd up' to start services");
    println!("  3. Create your admin account via the web UI");

    Ok(())
}
