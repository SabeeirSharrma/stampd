//! Outbound submission server (port 587).
//!
//! Requires AUTH — tokens or SMTP AUTH PLAIN/LOGIN over TLS.
//! Signs with DKIM and enqueues for delivery.

use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, warn, error};
use std::sync::Arc;
use std::net::SocketAddr;
use std::path::Path;

use crate::db::Database;
use crate::dkim::DkimSigner;

/// Per-session state for submission.
struct SubmissionSession {
    db: Arc<Database>,
    maildir_path: String,
    domain: String,
    dkim_selector: String,
    dkim_signer: Option<DkimSigner>,
    /// Authenticated user id (None until AUTH succeeds)
    authenticated_user_id: Option<i64>,
    /// Sender from MAIL FROM
    mail_from: Option<String>,
    /// Recipients
    rcpt_to: Vec<String>,
}

pub async fn run(
    port: u16,
    dkim_selector: String,
    db: Arc<Database>,
    dkim_signer: Option<DkimSigner>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!(port, dkim = dkim_signer.is_some(), "Submission server listening");

    // Get domain from server_config
    let (domain, _, _) = db.get_server_config()
        .unwrap_or(("localhost".to_string(), true, "default".to_string()));

    loop {
        let (stream, addr) = listener.accept().await?;
        info!(addr = %addr, "New submission connection");

        let db = db.clone();
        let dkim_selector = dkim_selector.clone();
        let domain = domain.clone();
        let dkim_signer = dkim_signer.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_submission(stream, addr, db, dkim_selector, domain, dkim_signer).await {
                error!(addr = %addr, error = ?e, "Submission error");
            }
        });
    }
}

async fn handle_submission(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    db: Arc<Database>,
    dkim_selector: String,
    domain: String,
    dkim_signer: Option<DkimSigner>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    let mut session = SubmissionSession {
        db,
        maildir_path: "/var/lib/stampd/mail".to_string(), // TODO: pass from config
        domain: domain.clone(),
        dkim_selector,
        dkim_signer,
        authenticated_user_id: None,
        mail_from: None,
        rcpt_to: Vec::new(),
    };

    // Greeting
    writer
        .write_all(b"220 Stampd Submission ready\r\n")
        .await?;

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break;
        }

        let cmd_line = line.trim_end_matches("\r\n").to_string();
        if cmd_line.is_empty() {
            continue;
        }

        info!(addr = %addr, cmd = %cmd_line, "Submission");

        let (verb, args) = match cmd_line.split_once(' ') {
            Some((v, a)) => (v.to_uppercase(), Some(a.to_string())),
            None => (cmd_line.to_uppercase(), None),
        };

        let response = match verb.as_str() {
            "HELO" | "EHLO" => {
                if verb == "EHLO" {
                    format!(
                        "250-{}\r\n250-8BITMIME\r\n250-AUTH PLAIN LOGIN\r\n250-STARTTLS\r\n250 SIZE 52428800\r\n",
                        domain
                    )
                } else {
                    format!("250 {}\r\n", domain)
                }
            }
            "AUTH" => {
                match handle_auth(&args.unwrap_or_default(), &session.db) {
                    Ok(user_id) => {
                        session.authenticated_user_id = Some(user_id);
                        info!(addr = %addr, user_id, "Authentication successful");
                        "235 Authentication successful\r\n".to_string()
                    }
                    Err(e) => {
                        warn!(addr = %addr, error = %e, "Authentication failed");
                        format!("535 {}\r\n", e)
                    }
                }
            }
            "STARTTLS" => {
                // TODO: Phase 0.2.3 — implement TLS upgrade
                "454 TLS not available\r\n".to_string()
            }
            "MAIL" => {
                if session.authenticated_user_id.is_none() {
                    "530 Authentication required\r\n".to_string()
                } else {
                    match parse_address(&args.unwrap_or_default()) {
                        Ok(sender) => {
                            session.mail_from = Some(sender.clone());
                            session.rcpt_to.clear();
                            info!(addr = %addr, sender = %sender, "MAIL FROM accepted");
                            "250 OK\r\n".to_string()
                        }
                        Err(e) => format!("501 {}\r\n", e),
                    }
                }
            }
            "RCPT" => {
                if session.authenticated_user_id.is_none() {
                    "530 Authentication required\r\n".to_string()
                } else {
                    match parse_address(&args.unwrap_or_default()) {
                        Ok(recipient) => {
                            // Validate: recipients must be external (not our domain)
                            let rcpt_domain = recipient.split('@').nth(1).unwrap_or("");
                            if rcpt_domain.eq_ignore_ascii_case(&session.domain) {
                                "550 Cannot send to local users via submission\r\n".to_string()
                            } else {
                                session.rcpt_to.push(recipient.clone());
                                info!(addr = %addr, recipient = %recipient, "RCPT TO accepted");
                                "250 OK\r\n".to_string()
                            }
                        }
                        Err(e) => format!("501 {}\r\n", e),
                    }
                }
            }
            "DATA" => {
                if session.authenticated_user_id.is_none() {
                    "530 Authentication required\r\n".to_string()
                } else if session.mail_from.is_none() || session.rcpt_to.is_empty() {
                    "503 Bad sequence of commands\r\n".to_string()
                } else {
                    writer.write_all(b"354 Start mail input; end with <CRLF>.<CRLF>\r\n").await?;

                    let message = read_message_body(&mut reader, &mut line).await;

                    // Store the message to a temp file and enqueue for each recipient
                    match enqueue_message(&session, &message).await {
                        Ok(count) => {
                            info!(
                                addr = %addr,
                                recipients = count,
                                "Message enqueued for delivery"
                            );
                            session.mail_from = None;
                            session.rcpt_to.clear();
                            "250 OK: Message enqueued for delivery\r\n".to_string()
                        }
                        Err(e) => {
                            error!(addr = %addr, error = ?e, "Failed to enqueue message");
                            "451 Local error in processing\r\n".to_string()
                        }
                    }
                }
            }
            "RSET" => {
                session.mail_from = None;
                session.rcpt_to.clear();
                "250 OK\r\n".to_string()
            }
            "QUIT" => {
                writer.write_all(b"221 Bye\r\n").await?;
                break;
            }
            "NOOP" => {
                "250 OK\r\n".to_string()
            }
            _ => {
                warn!(addr = %addr, verb = %verb, "Unknown command");
                "500 Command not recognized\r\n".to_string()
            }
        };

        writer.write_all(response.as_bytes()).await?;
    }

    info!(addr = %addr, "Submission connection closed");
    Ok(())
}

// ── AUTH Handling ────────────────────────────────────────────────

/// Handle AUTH PLAIN or AUTH LOGIN.
fn handle_auth(args: &str, db: &Database) -> Result<i64, String> {
    let args = args.trim();

    if args.starts_with("PLAIN ") {
        let encoded = &args[6..];
        let decoded = base64_decode(encoded)
            .ok_or("Invalid base64 encoding")?;

        // AUTH PLAIN format: \0username\0password
        let parts: Vec<&[u8]> = decoded.split(|&b| b == 0).collect();
        if parts.len() < 3 {
            return Err("Invalid AUTH PLAIN format".to_string());
        }

        let username = String::from_utf8_lossy(parts[1]);
        let password = String::from_utf8_lossy(parts[2]);

        authenticate_user(&username, &password, db)
    } else if args.starts_with("LOGIN ") {
        // AUTH LOGIN: first token is base64(username), then base64(password)
        // For simplicity, we handle the first part here; the second comes in the next command
        // TODO: implement multi-step AUTH LOGIN
        Err("AUTH LOGIN not yet supported, use AUTH PLAIN".to_string())
    } else {
        Err("Unsupported AUTH mechanism".to_string())
    }
}

/// Authenticate a user by email and password.
fn authenticate_user(email: &str, password: &str, db: &Database) -> Result<i64, String> {
    let (user_id, password_hash, _is_admin, disabled) = db.get_user_by_email(email)
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or("Invalid credentials".to_string())?;

    if disabled {
        return Err("Account is disabled".to_string());
    }

    // Verify password with argon2id
    // For now, do a simple hash comparison (placeholder — replace with real argon2)
    if verify_password(password, &password_hash) {
        Ok(user_id)
    } else {
        Err("Invalid credentials".to_string())
    }
}

/// Verify a password against its hash.
///
/// TODO: Replace with real argon2id verification.
fn verify_password(password: &str, hash: &str) -> bool {
    // Placeholder: simple equality check for development
    // In production, use argon2 crate
    format!("hash:{}", password) == hash
}

// ── Address Parsing ──────────────────────────────────────────────

/// Parse an email address from MAIL FROM:<addr> or RCPT TO:<addr> syntax.
fn parse_address(args: &str) -> Result<String, String> {
    let args = args.trim();
    if let Some(start) = args.find('<') {
        if let Some(end) = args.find('>') {
            let addr = &args[start + 1..end];
            if addr.is_empty() {
                return Err("Address cannot be empty".to_string());
            }
            // Basic format check
            if !addr.contains('@') {
                return Err("Invalid address format".to_string());
            }
            return Ok(addr.to_string());
        }
    }
    // Try without angle brackets
    let addr = args.split_whitespace().next().unwrap_or("");
    if addr.contains('@') {
        Ok(addr.to_string())
    } else {
        Err("MAIL FROM requires <address> syntax".to_string())
    }
}

// ── Message Body Reading ─────────────────────────────────────────

/// Read the message body until dot-stuffing terminator.
async fn read_message_body(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
    line: &mut String,
) -> Vec<u8> {
    let mut body = Vec::new();

    loop {
        line.clear();
        if reader.read_line(line).await.unwrap_or(0) == 0 {
            break;
        }

        let raw_line = line.as_bytes();
        let trimmed = line.trim_end_matches("\r\n");

        if trimmed == "." {
            break;
        }

        if trimmed.starts_with("..") {
            body.extend_from_slice(&raw_line[1..]);
        } else {
            body.extend_from_slice(raw_line);
        }
    }

    body
}

// ── Enqueue for Delivery ─────────────────────────────────────────

/// Enqueue the message for delivery to all recipients.
///
/// Signs with DKIM before enqueuing if DKIM signer is available.
async fn enqueue_message(
    session: &SubmissionSession,
    message: &[u8],
) -> anyhow::Result<usize> {
    let user_id = session.authenticated_user_id
        .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    // Build the full .eml with envelope headers
    let mut full_message = Vec::new();
    full_message.extend_from_slice(format!("X-Stampd-From: {}\r\n", session.mail_from.as_deref().unwrap_or("")).as_bytes());
    full_message.extend_from_slice(format!("X-Stampd-To: {}\r\n", session.rcpt_to.join(", ")).as_bytes());
    full_message.extend_from_slice(b"\r\n");
    full_message.extend_from_slice(message);

    // Sign with DKIM if available
    let signed_message = if let Some(ref signer) = session.dkim_signer {
        let msg_str = String::from_utf8_lossy(&full_message);
        match signer.sign_message(&msg_str) {
            Ok(signed) => {
                info!("DKIM signing successful");
                signed.into_bytes()
            }
            Err(e) => {
                warn!(error = ?e, "DKIM signing failed, sending unsigned");
                full_message
            }
        }
    } else {
        full_message
    };

    let mut count = 0;

    for recipient in &session.rcpt_to {
        // Save message to a temp file for the queue processor
        let msg_filename = format!(
            "out-{}.{}.eml",
            timestamp,
            recipient.replace('@', "_at_").replace('.', "_")
        );
        let msg_path = Path::new("/var/lib/stampd/outbox").join(&msg_filename);

        // Ensure outbox directory exists
        if let Some(parent) = msg_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&msg_path, &signed_message).await?;

        // Enqueue in database
        session.db.enqueue(
            user_id,
            recipient,
            msg_path.to_str().unwrap_or(""),
        )?;
        count += 1;
    }

    Ok(count)
}

// ── Base64 Decode (simple) ──────────────────────────────────────

/// Simple base64 decoder for AUTH.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    // Simple base64 decode without external crate
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = input.trim_end_matches('=');
    let mut result = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &byte in input.as_bytes() {
        let val = CHARS.iter().position(|&c| c == byte)? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            result.push((buf >> bits) as u8);
        }
    }

    Some(result)
}
