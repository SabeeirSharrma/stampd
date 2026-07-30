use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct StampdConfig {
    pub engine: Option<EngineSection>,
    pub gateway: Option<ServiceSection>,
    pub admin: Option<ServiceSection>,
    pub web: Option<ServiceSection>,
    pub filters: Option<FiltersSection>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct EngineSection {
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    #[serde(default = "default_submission_port")]
    pub submission_port: u16,
    #[serde(default = "default_maildir_path")]
    pub maildir_path: String,
    #[serde(default = "default_dkim_selector")]
    pub dkim_selector: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ServiceSection {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct FiltersSection {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

#[allow(dead_code)]
fn default_smtp_port() -> u16 {
    25
}
#[allow(dead_code)]
fn default_submission_port() -> u16 {
    587
}
#[allow(dead_code)]
fn default_maildir_path() -> String {
    stampd_data_dir()
        .join("mail")
        .to_string_lossy()
        .into_owned()
}
#[allow(dead_code)]
fn default_dkim_selector() -> String {
    "default".to_string()
}
#[allow(dead_code)]
fn default_true() -> bool {
    true
}
#[allow(dead_code)]
fn default_timeout_ms() -> u64 {
    500
}

#[allow(dead_code)]
pub fn stampd_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("stampd")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/var/lib/stampd")
    }
}

#[allow(dead_code)]
pub fn default_pid_dir() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::runtime_dir()
            .or_else(|| dirs::data_local_dir())
            .unwrap_or_else(|| PathBuf::from("."))
            .join("stampd")
            .join("run")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/var/run/stampd")
    }
}

#[allow(dead_code)]
pub fn default_log_dir() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("stampd")
            .join("logs")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/var/log/stampd")
    }
}

fn engine_binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "stampd-engine.exe"
    }
    #[cfg(not(windows))]
    {
        "stampd-engine"
    }
}

fn find_engine_binary() -> String {
    // Try to find stampd-engine next to the current executable first
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(engine_binary_name());
            if candidate.exists() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }
    // Fall back to relative path
    #[cfg(windows)]
    {
        ".\\target\\debug\\stampd-engine.exe".to_string()
    }
    #[cfg(not(windows))]
    {
        "./target/debug/stampd-engine".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub enabled: bool,
}

impl StampdConfig {
    pub fn build_service_list(&self, only: Option<Vec<String>>) -> Vec<ServiceConfig> {
        let mut services = Vec::new();

        let is_enabled = |name: &str| -> bool {
            only.as_ref()
                .map(|o| o.iter().any(|s| s == name))
                .unwrap_or_else(|| {
                    match name {
                        "engine" => true, // Engine is always enabled if present
                        "gateway" => self.gateway.as_ref().is_some_and(|g| g.enabled),
                        "admin" => self.admin.as_ref().is_some_and(|a| a.enabled),
                        "web" => self.web.as_ref().is_some_and(|w| w.enabled),
                        _ => false,
                    }
                })
        };

        if is_enabled("engine") {
            services.push(ServiceConfig {
                name: "engine".to_string(),
                command: find_engine_binary(),
                args: vec!["stampd.toml".to_string()],
                enabled: true,
            });
        }

        if is_enabled("gateway") {
            let (cmd, cmd_args) = if cfg!(windows) {
                ("bun.cmd".to_string(), vec!["run".to_string()])
            } else {
                ("bun".to_string(), vec!["run".to_string()])
            };
            services.push(ServiceConfig {
                name: "gateway".to_string(),
                command: cmd,
                args: [cmd_args, vec!["stampd-gateway/src/index.ts".to_string()]].concat(),
                enabled: true,
            });
        }

        if is_enabled("admin") {
            let (cmd, host_args) = if cfg!(windows) {
                ("python".to_string(), vec![])
            } else {
                ("python3".to_string(), vec![])
            };
            services.push(ServiceConfig {
                name: "admin".to_string(),
                command: cmd,
                args: [
                    host_args,
                    vec![
                        "-m".to_string(),
                        "uvicorn".to_string(),
                        "app.main:app".to_string(),
                        "--host".to_string(),
                        "0.0.0.0".to_string(),
                        "--port".to_string(),
                        "8081".to_string(),
                    ],
                ]
                .concat(),
                enabled: true,
            });
        }

        if is_enabled("web") {
            let (cmd, cmd_args) = if cfg!(windows) {
                ("bun.cmd".to_string(), vec!["run".to_string()])
            } else {
                ("bun".to_string(), vec!["run".to_string()])
            };
            services.push(ServiceConfig {
                name: "web".to_string(),
                command: cmd,
                args: [cmd_args, vec!["stampd-web/src/index.ts".to_string()]].concat(),
                enabled: true,
            });
        }

        services
    }
}

pub fn load_config(path: &str) -> Result<StampdConfig, anyhow::Error> {
    let content = std::fs::read_to_string(path)?;
    let config: StampdConfig = toml::from_str(&content)?;
    Ok(config)
}
