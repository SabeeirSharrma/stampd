use crate::config::ServiceConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

const MAX_RESTART_ATTEMPTS: u32 = 10;
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Supervisor {
    services: Vec<ServiceConfig>,
    children: HashMap<String, Child>,
    restart_counts: HashMap<String, u32>,
    pid_dir: PathBuf,
    log_dir: PathBuf,
}

impl Supervisor {
    pub fn new(pid_dir: PathBuf, log_dir: PathBuf) -> Self {
        // Ensure directories exist
        let _ = std::fs::create_dir_all(&pid_dir);
        let _ = std::fs::create_dir_all(&log_dir);

        Self {
            services: Vec::new(),
            children: HashMap::new(),
            restart_counts: HashMap::new(),
            pid_dir,
            log_dir,
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
        // Remove old PID file if exists
        let pid_file = self.pid_dir.join(format!("{}.pid", service.name));
        let _ = std::fs::remove_file(&pid_file);

        // Set up log files
        let stdout_log = self.log_dir.join(format!("{}.stdout.log", service.name));
        let stderr_log = self.log_dir.join(format!("{}.stderr.log", service.name));

        info!(
            service = %service.name,
            command = %service.command,
            args = ?service.args,
            "Starting service"
        );

        let stdout = std::fs::File::create(&stdout_log)
            .map_err(|e| anyhow::anyhow!("Failed to create stdout log: {}", e))?;
        let stderr = std::fs::File::create(&stderr_log)
            .map_err(|e| anyhow::anyhow!("Failed to create stderr log: {}", e))?;

        let mut child = Command::new(&service.command)
            .args(&service.args)
            .stdout(std::process::Stdio::from(stdout))
            .stderr(std::process::Stdio::from(stderr))
            .kill_on_drop(true)
            .spawn()?;

        let child_id = child.id().unwrap_or(0);

        // Write PID file
        std::fs::write(&pid_file, child_id.to_string())
            .map_err(|e| anyhow::anyhow!("Failed to write PID file: {}", e))?;

        info!(service = %service.name, pid = child_id, "Service started");

        // Spawn task to monitor this child
        let service_name = service.name.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            let success = status.map(|s| s.success()).unwrap_or(false);
            let _ = tx.send((service_name, success)).await;
        });

        Ok(())
    }

    async fn shutdown(&mut self) {
        info!("Shutting down all services...");

        // Send SIGTERM to all children
        for (name, child) in &mut self.children {
            if let Some(pid) = child.id() {
                info!(service = %name, pid = pid, "Sending SIGTERM");
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
        }

        // Wait for graceful shutdown
        let deadline = tokio::time::Instant::now() + SHUTDOWN_TIMEOUT;
        for (name, child) in &mut self.children {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                warn!("Shutdown timeout reached, force killing remaining services");
                break;
            }
            match tokio::time::timeout(remaining, child.wait()).await {
                Ok(Ok(_)) => info!(service = %name, "Service stopped gracefully"),
                Ok(Err(e)) => warn!(service = %name, error = ?e, "Service exited with error"),
                Err(_) => {
                    warn!(service = %name, "Service did not stop in time, force killing");
                    let _ = child.kill().await;
                }
            }
        }

        // Clean up PID files
        for service in &self.services {
            let pid_file = self.pid_dir.join(format!("{}.pid", service.name));
            let _ = std::fs::remove_file(&pid_file);
        }

        info!("All services stopped");
    }

    /// Stop all services
    pub async fn stop_all(&mut self) -> Result<(), anyhow::Error> {
        // Read PID files and kill processes
        for service in &self.services {
            let pid_file = self.pid_dir.join(format!("{}.pid", service.name));
            if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    info!(service = %service.name, pid = pid, "Stopping service");
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                    // Wait briefly for graceful shutdown
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    // Force kill if still running
                    unsafe {
                        libc::kill(pid as i32, libc::SIGKILL);
                    }
                }
            }
            let _ = std::fs::remove_file(&pid_file);
        }
        Ok(())
    }

    /// Check status of all services
    pub fn check_status(&self) -> Vec<(String, ServiceStatus)> {
        let mut statuses = Vec::new();
        for service in &self.services {
            let pid_file = self.pid_dir.join(format!("{}.pid", service.name));
            let status = if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    // Check if process is still running
                    if is_process_running(pid) {
                        ServiceStatus::Running(pid)
                    } else {
                        ServiceStatus::Stopped
                    }
                } else {
                    ServiceStatus::Unknown
                }
            } else {
                ServiceStatus::Stopped
            };
            statuses.push((service.name.clone(), status));
        }
        statuses
    }
}

pub enum ServiceStatus {
    Running(u32),
    Stopped,
    Unknown,
}

fn is_process_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn calculate_backoff(attempt: u32) -> Duration {
    let backoff = INITIAL_BACKOFF * 2u32.pow(attempt.min(4));
    backoff.min(MAX_BACKOFF)
}
