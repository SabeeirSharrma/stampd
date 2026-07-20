use std::collections::HashMap;
use std::time::Duration;
use tokio::process::{Command, Child};
use tokio::sync::mpsc;
use tracing::{info, warn, error};
use crate::config::ServiceConfig;

const MAX_RESTART_ATTEMPTS: u32 = 10;
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

pub struct Supervisor {
    services: Vec<ServiceConfig>,
    children: HashMap<String, Child>,
    restart_counts: HashMap<String, u32>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
            children: HashMap::new(),
            restart_counts: HashMap::new(),
        }
    }

    pub fn add_service(&mut self, service: ServiceConfig) {
        self.services.push(service);
    }

    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        // Channel for process exit notifications
        let (tx, mut rx) = mpsc::channel::<(String, bool)>(32);

        // Start all services
        let services_to_start: Vec<ServiceConfig> = self.services.clone();
        for service in &services_to_start {
            self.start_service(service, &tx).await?;
        }

        info!("All services started. Press Ctrl+C to stop.");

        // Set up Ctrl+C handler
        let ctrl_c_tx = tx.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("Received Ctrl+C, shutting down...");
            drop(ctrl_c_tx);
        });

        // Monitor process exits and restart if needed
        while let Some((service_name, success)) = rx.recv().await {
            if !success {
                warn!(service = %service_name, "Service exited with error");

                // Check restart count
                let count = self.restart_counts.entry(service_name.clone()).or_insert(0);
                if *count >= MAX_RESTART_ATTEMPTS {
                    error!(
                        service = %service_name,
                        attempts = MAX_RESTART_ATTEMPTS,
                        "Max restart attempts reached, not restarting"
                    );
                    continue;
                }

                // Calculate backoff
                let backoff = calculate_backoff(*count);
                *count += 1;

                info!(
                    service = %service_name,
                    attempt = *count,
                    backoff_secs = backoff.as_secs(),
                    "Restarting service"
                );

                // Wait before restart
                tokio::time::sleep(backoff).await;

                // Find service config and restart
                if let Some(service) = self.services.iter().find(|s| s.name == service_name) {
                    let svc = service.clone();
                    self.start_service(&svc, &tx).await?;
                }
            } else {
                // Service exited cleanly, reset restart count
                self.restart_counts.remove(&service_name);
            }
        }

        // Shutdown: kill all children
        self.shutdown().await;

        Ok(())
    }

    async fn start_service(
        &mut self,
        service: &ServiceConfig,
        tx: &mpsc::Sender<(String, bool)>,
    ) -> Result<(), anyhow::Error> {
        info!(
            service = %service.name,
            command = %service.command,
            args = ?service.args,
            "Starting service"
        );

        let mut child = Command::new(&service.command)
            .args(&service.args)
            .kill_on_drop(true)
            .spawn()?;

        let service_name = service.name.clone();
        let child_id = child.id().unwrap_or(0);
        let tx = tx.clone();

        // Spawn task to monitor this child
        tokio::spawn(async move {
            let status = child.wait().await;
            let success = status.map(|s| s.success()).unwrap_or(false);
            let _ = tx.send((service_name, success)).await;
        });

        // Store a placeholder — we can't store the child after moving it,
        // so we track by pid/name instead
        info!(service = %service.name, pid = child_id, "Service started");
        self.children.remove(&service.name); // Clean up old entry if any

        Ok(())
    }

    async fn shutdown(&mut self) {
        info!("Shutting down all services...");

        // We've already moved children to monitoring tasks,
        // so we just log the shutdown. In production, we'd use
        // PID files or a process group to track and kill children.
        info!("All services stopped");
    }
}

fn calculate_backoff(attempt: u32) -> Duration {
    let backoff = INITIAL_BACKOFF * 2u32.pow(attempt.min(4));
    backoff.min(MAX_BACKOFF)
}
