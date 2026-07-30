use serde::Deserialize;

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

fn default_smtp_port() -> u16 {
    25
}
fn default_submission_port() -> u16 {
    587
}
fn default_maildir_path() -> String {
    "/var/lib/stampd/mail".to_string()
}
fn default_dkim_selector() -> String {
    "default".to_string()
}
fn default_true() -> bool {
    true
}
fn default_timeout_ms() -> u64 {
    500
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
                command: "python3".to_string(),
                args: vec![
                    "-m".to_string(),
                    "uvicorn".to_string(),
                    "app.main:app".to_string(),
                    "--host".to_string(),
                    "0.0.0.0".to_string(),
                    "--port".to_string(),
                    "8081".to_string(),
                ],
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
}

pub fn load_config(path: &str) -> Result<StampdConfig, anyhow::Error> {
    let content = std::fs::read_to_string(path)?;
    let config: StampdConfig = toml::from_str(&content)?;
    Ok(config)
}
