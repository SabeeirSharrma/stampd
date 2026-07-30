//! DKIM signing for outgoing messages (RFC 6376).
//!
//! Signs messages with RSA key pair, adds DKIM-Signature header.
//! Keys stored in PKCS8 format in config directory.
//! Use `openssl genrsa -out key.pem 2048 && openssl pkcs8 -topk8 -inform PEM -outform DER -in key.pem -out key.pkcs8 -nocrypt` to generate.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ring::digest;
use ring::rsa::KeyPair;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

/// DKIM signer that holds the current key pair.
#[derive(Clone)]
pub struct DkimSigner {
    key_pair: Arc<KeyPair>,
    selector: String,
    domain: String,
}

impl DkimSigner {
    /// Create a new DKIM signer, loading an existing PKCS8 key.
    ///
    /// Generate a key with:
    /// ```sh
    /// openssl genrsa -out default.pem 2048
    /// openssl pkcs8 -topk8 -inform PEM -outform DER -in default.pem -out default.pkcs8 -nocrypt
    /// ```
    pub fn new(domain: &str, selector: &str, key_dir: &Path) -> anyhow::Result<Self> {
        let pkcs8_path = key_dir.join(format!("{}.pkcs8", selector));

        if !pkcs8_path.exists() {
            return Err(anyhow::anyhow!(
                "DKIM key not found at {}. Generate with:\n\
                 openssl genrsa -out {}/{}.pem 2048\n\
                 openssl pkcs8 -topk8 -inform PEM -outform DER -in {}/{}.pem -out {} -nocrypt",
                pkcs8_path.display(),
                key_dir.display(),
                selector,
                key_dir.display(),
                selector,
                pkcs8_path.display()
            ));
        }

        info!(selector = %selector, path = %pkcs8_path.display(), "Loading DKIM key");
        let pkcs8_der = std::fs::read(&pkcs8_path)?;
        let key_pair = KeyPair::from_pkcs8(&pkcs8_der)
            .map_err(|e| anyhow::anyhow!("Failed to load DKIM key: {:?}", e))?;

        // Save public key in DNS format if not already present
        let dns_path = key_dir.join(format!("{}.dns.txt", selector));
        if !dns_path.exists() {
            let pubkey_dns = format!(
                "v=DKIM1; k=rsa; p={}",
                BASE64.encode(key_pair.public().as_ref())
            );
            std::fs::write(&dns_path, &pubkey_dns)?;
            info!(selector = %selector, pubkey = %pubkey_dns, "DKIM public key for DNS (saved to {})", dns_path.display());
        }

        Ok(Self {
            key_pair: Arc::new(key_pair),
            selector: selector.to_string(),
            domain: domain.to_string(),
        })
    }

    /// Sign a message and return the DKIM-Signature header value.
    pub fn sign(&self, raw_message: &str) -> anyhow::Result<String> {
        let (headers, body) = match raw_message.find("\r\n\r\n") {
            Some(pos) => (&raw_message[..pos], &raw_message[pos + 4..]),
            None => (raw_message, ""),
        };

        // Headers to sign
        let signed_header_names = vec!["from", "to", "subject", "date", "message-id"];

        let mut found_headers = Vec::new();
        for header_name in &signed_header_names {
            for line in headers.lines() {
                if line
                    .to_lowercase()
                    .starts_with(&format!("{}:", header_name))
                {
                    found_headers.push(header_name.to_string());
                    break;
                }
            }
        }

        let header_list = found_headers.join(": ");

        // Canonicalize headers (relaxed)
        let mut canonical_headers = Vec::new();
        for line in headers.lines() {
            let lower = line.to_lowercase();
            for header_name in &found_headers {
                if lower.starts_with(&format!("{}:", header_name)) {
                    if let Some(colon_pos) = line.find(':') {
                        let name = line[..colon_pos].to_lowercase();
                        let value = line[colon_pos + 1..].trim();
                        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
                        canonical_headers.push(format!("{}:{}", name, value));
                    }
                    break;
                }
            }
        }

        // Canonicalize body (relaxed)
        let canonical_body = if body.is_empty() {
            "\r\n".to_string()
        } else {
            let trimmed = body.trim_end();
            if trimmed.is_empty() {
                "\r\n".to_string()
            } else {
                format!("{}\r\n", trimmed)
            }
        };

        let headers_str = canonical_headers.join("\r\n");
        let body_hash = digest::digest(&digest::SHA256, canonical_body.as_bytes());
        let body_b64 = BASE64.encode(body_hash.as_ref());

        // Build DKIM-Signature header (without b= tag for signing)
        let mut signature_header = format!(
            "DKIM-Signature; v=1; a=rsa-sha256; d={}; s={}; c=relaxed/relaxed; q=dns/txt; h={}; bh={}; b=",
            self.domain,
            self.selector,
            header_list,
            body_b64,
        );

        // Data to sign: header (with empty b=) + CRLF + canonical headers
        let data_to_sign = format!("{}{}", signature_header, headers_str);

        // Sign with RSA-SHA256
        let rng = ring::rand::SystemRandom::new();
        let mut signature = vec![0u8; self.key_pair.public().modulus_len()];
        self.key_pair
            .sign(
                &ring::signature::RSA_PKCS1_SHA256,
                &rng,
                data_to_sign.as_bytes(),
                &mut signature,
            )
            .map_err(|e| anyhow::anyhow!("DKIM signing failed: {:?}", e))?;

        let sig_b64 = BASE64.encode(&signature);
        signature_header.push_str(&sig_b64);

        Ok(signature_header)
    }

    /// Insert DKIM-Signature header into a raw message.
    pub fn sign_message(&self, message: &str) -> anyhow::Result<String> {
        let dkim_header = self.sign(message)?;

        if let Some(pos) = message.find("\r\n\r\n") {
            let mut result = String::with_capacity(message.len() + dkim_header.len() + 10);
            result.push_str(&message[..pos]);
            result.push_str("\r\n");
            result.push_str(&dkim_header);
            result.push_str(&message[pos..]);
            Ok(result)
        } else {
            Ok(format!("{}\r\n\r\n", dkim_header))
        }
    }
}

/// Get the DKIM public key record for DNS setup.
pub fn get_dkim_dns_record(
    _domain: &str,
    selector: &str,
    key_dir: &Path,
) -> anyhow::Result<String> {
    let dns_path = key_dir.join(format!("{}.dns.txt", selector));
    if dns_path.exists() {
        Ok(std::fs::read_to_string(dns_path)?)
    } else {
        Err(anyhow::anyhow!(
            "DKIM key not found for selector {}",
            selector
        ))
    }
}
