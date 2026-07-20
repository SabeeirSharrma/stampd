use std::path::PathBuf;
use tokio::process::Command;
use tracing::info;
use anyhow::Result;

#[derive(Debug)]
struct ServiceConfig {
    name: String,
    command: String,
    args: Vec<String>,
    enabled: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
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
            println!("  stampd up                    Start all enabled services");
            println!("  stampd up --only engine,gateway  Start specific services");
            println!("  stampd status                Show running services");
        }
    }

    Ok(())
}

async fn cmd_up(only: Option<Vec<String>>) -> Result<()> {
    let config_path = PathBuf::from("stampd.toml");
    let config_content = std::fs::read_to_string(&config_path)?;
    let config: toml::Value = toml::from_str(&config_content)?;

    let services = build_service_list(&config, only);
    info!(services = ?services.iter().map(|s| &s.name).collect::<Vec<_>>(), "Starting services");

    let mut handles = Vec::new();

    for service in &services {
        if !service.enabled {
            info!(service = %service.name, "Skipping disabled service");
            continue;
        }

        info!(service = %service.name, command = %service.command, args = ?service.args, "Starting service");

        let child = Command::new(&service.command)
            .args(&service.args)
            .kill_on_drop(true)
            .spawn()?;

        handles.push((service.name.clone(), child));
    }

    // Wait for any process to exit
    if !handles.is_empty() {
        info!("All services started. Press Ctrl+C to stop.");
        tokio::signal::ctrl_c().await?;

        info!("Shutting down...");
        // Drop handles to kill processes
        for (name, mut child) in handles {
            info!(service = %name, "Stopping");
            child.kill().await?;
        }
    }

    Ok(())
}

async fn cmd_status() -> Result<()> {
    println!("stampd status — not yet implemented");
    println!("TODO: Check running processes");
    Ok(())
}

fn build_service_list(config: &toml::Value, only: Option<Vec<String>>) -> Vec<ServiceConfig> {
    let mut services = Vec::new();

    let is_enabled = |name: &str| -> bool {
        only.as_ref()
            .map(|o| o.iter().any(|s| s == name))
            .unwrap_or_else(|| {
                config
                    .get(name)
                    .and_then(|s| s.get("enabled"))
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false)
            })
    };

    if is_enabled("engine") {
        services.push(ServiceConfig {
            name: "engine".to_string(),
            command: "./target/debug/stampd-engine".to_string(),
            args: vec!["stampd.toml".to_string()],
            enabled: true,
        });
    }

    if is_enabled("gateway") {
        services.push(ServiceConfig {
            name: "gateway".to_string(),
            command: "bun".to_string(),
            args: vec!["run".to_string(), "stampd-gateway/src/index.ts".to_string()],
            enabled: true,
        });
    }

    if is_enabled("admin") {
        services.push(ServiceConfig {
            name: "admin".to_string(),
            command: "python".to_string(),
            args: vec!["-m".to_string(), "uvicorn".to_string(), "stampd_admin.app:app".to_string()],
            enabled: true,
        });
    }

    if is_enabled("web") {
        services.push(ServiceConfig {
            name: "web".to_string(),
            command: "bun".to_string(),
            args: vec!["run".to_string(), "stampd-web/src/index.ts".to_string()],
            enabled: true,
        });
    }

    services
}
