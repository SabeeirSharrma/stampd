mod supervisor;
mod config;

use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing with service-aware formatting
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("up") => {
            let only: Option<Vec<String>> = args.get(2)
                .filter(|s| s.as_str() == "--only")
                .and_then(|_| args.get(3))
                .map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

            cmd_up(only).await?;
        }
        Some("status") => {
            cmd_status().await?;
        }
        _ => {
            println!("stampd — self-hosted mail server");
            println!();
            println!("Usage:");
            println!("  stampd up                         Start all enabled services");
            println!("  stampd up --only engine,gateway   Start specific services");
            println!("  stampd status                     Show running services");
        }
    }

    Ok(())
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

    let mut sup = supervisor::Supervisor::new();

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

async fn cmd_status() -> Result<()> {
    let config = config::load_config("stampd.toml")?;
    let services = config.build_service_list(None);

    println!("Stampd Service Status");
    println!("=====================");
    println!();

    for service in &services {
        let status = check_service_status(service);
        let indicator = match status {
            ServiceStatus::Running(pid) => format!("✓ running (pid {})", pid),
            ServiceStatus::Stopped => "✗ stopped".to_string(),
            ServiceStatus::Unknown => "? unknown".to_string(),
        };
        println!("  {:<12} {}", service.name, indicator);
    }

    println!();
    Ok(())
}

enum ServiceStatus {
    Running(u32),
    Stopped,
    Unknown,
}

fn check_service_status(service: &config::ServiceConfig) -> ServiceStatus {
    // For Rust binaries, check if the process is running
    // This is a basic implementation — in production, use PID files or socket checks
    match service.name.as_str() {
        "engine" | "cli" => {
            // Check if port is listening (basic health check)
            ServiceStatus::Unknown
        }
        "gateway" => {
            // Check HTTP health endpoint
            ServiceStatus::Unknown
        }
        "admin" => {
            // Check HTTP health endpoint
            ServiceStatus::Unknown
        }
        _ => ServiceStatus::Unknown,
    }
}
