use serde::Deserialize;
use std::path::{Path, PathBuf};

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
    /// Directory containing filter scripts.
    #[serde(default = "default_filters_dir")]
    pub filters_dir: String,
    /// Timeout for filter execution in milliseconds.
    #[serde(default = "default_filters_timeout_ms")]
    pub filters_timeout_ms: u64,
    /// Gateway URL for filter delegation via Transit (optional).
    /// When set, filters are executed via the gateway's Transit Python bridge
    /// instead of spawning scripts directly.
    #[serde(default)]
    pub gateway_url: Option<String>,
}

fn stampd_data_dir() -> PathBuf {
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

fn default_domain() -> String {
    "localhost".to_string()
}

fn default_db_path() -> String {
    stampd_data_dir()
        .join("stampd.db")
        .to_string_lossy()
        .into_owned()
}

fn default_api_port() -> u16 {
    8090
}

fn default_dkim_key_dir() -> String {
    stampd_data_dir()
        .join("dkim")
        .to_string_lossy()
        .into_owned()
}

fn default_filters_dir() -> String {
    stampd_data_dir()
        .join("filters")
        .to_string_lossy()
        .into_owned()
}

fn default_filters_timeout_ms() -> u64 {
    500
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
