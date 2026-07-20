use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub engine: EngineConfig,
}

#[derive(Debug, Deserialize)]
pub struct EngineConfig {
    pub smtp_port: u16,
    pub submission_port: u16,
    pub maildir_path: String,
    pub dkim_selector: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default = "default_db_path")]
    pub db_path: String,
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    /// TLS certificate path for STARTTLS (optional, generates self-signed if missing).
    #[serde(default)]
    pub tls_cert_path: Option<String>,
    /// TLS private key path for STARTTLS (optional, generates self-signed if missing).
    #[serde(default)]
    pub tls_key_path: Option<String>,
    /// Directory to store DKIM keys.
    #[serde(default = "default_dkim_key_dir")]
    pub dkim_key_dir: String,
}

fn default_domain() -> String {
    "localhost".to_string()
}

fn default_db_path() -> String {
    "/var/lib/stampd/stampd.db".to_string()
}

fn default_api_port() -> u16 {
    8090
}

fn default_dkim_key_dir() -> String {
    "/var/lib/stampd/dkim".to_string()
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
