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
}

fn default_domain() -> String {
    "localhost".to_string()
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
