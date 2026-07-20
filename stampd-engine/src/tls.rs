//! TLS configuration for STARTTLS support.
//!
//! Loads certificate and private key from PEM files.
//! If no files are configured, STARTTLS is disabled.

use std::path::Path;
use std::sync::Arc;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tracing::{info, warn};

/// TLS config holder for the SMTP server.
#[derive(Clone)]
pub struct TlsConfig {
    pub server_config: Arc<ServerConfig>,
}

impl TlsConfig {
    /// Load TLS config from cert/key files.
    pub fn load(cert_path: &Path, key_path: &Path) -> anyhow::Result<Self> {
        // Install the default crypto provider if not already installed
        let _ = rustls::crypto::ring::default_provider().install_default();

        info!(cert = %cert_path.display(), key = %key_path.display(), "Loading TLS certificate");

        let certs = load_cert_file(cert_path)?;
        let key = load_key_file(key_path)?;

        let mut server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| anyhow::anyhow!("TLS config error: {}", e))?;

        // ALPN not needed for SMTP STARTTLS, but set it anyway
        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        Ok(Self {
            server_config: Arc::new(server_config),
        })
    }
}

fn load_cert_file(path: &Path) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let cert_file = std::fs::read(path)?;
    let mut reader = std::io::Cursor::new(cert_file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        return Err(anyhow::anyhow!("No certificates found in {}", path.display()));
    }
    Ok(certs)
}

fn load_key_file(path: &Path) -> anyhow::Result<PrivateKeyDer<'static>> {
    let key_file = std::fs::read(path)?;
    let mut reader = std::io::Cursor::new(key_file);
    let key = rustls_pemfile::private_key(&mut reader)?
        .ok_or_else(|| anyhow::anyhow!("No private key found in {}", path.display()))?;
    Ok(key)
}

/// Try to load TLS config. Returns None if files don't exist (STARTTLS disabled).
pub fn try_load(cert_path: Option<&Path>, key_path: Option<&Path>) -> Option<TlsConfig> {
    match (cert_path, key_path) {
        (Some(cert), Some(key)) => {
            if cert.exists() && key.exists() {
                match TlsConfig::load(cert, key) {
                    Ok(tc) => {
                        info!("TLS loaded successfully");
                        Some(tc)
                    }
                    Err(e) => {
                        warn!(error = ?e, "Failed to load TLS — STARTTLS disabled");
                        None
                    }
                }
            } else {
                info!("TLS cert/key files not found — STARTTLS disabled");
                None
            }
        }
        _ => {
            info!("No TLS cert configured — STARTTLS disabled");
            None
        }
    }
}
