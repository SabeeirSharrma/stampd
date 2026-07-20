//! Inbound SMTP server (port 25).
//!
//! RFC 5321 command handling: HELO/EHLO, MAIL FROM, RCPT TO, DATA, RSET, QUIT.
//! No auth required for inbound (standard internet behavior).
//! Rejects any RCPT TO not addressed to the server's configured domain.

use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, warn, error};
use std::sync::Arc;
use std::net::SocketAddr;

use crate::db::Database;

/// Per-session state during an SMTP transaction.
struct SmtpSession {
    domain: String,
    maildir_path: String,
    db: Arc<Database>,
    /// Client's claimed domain (from HELO/EHLO)
    helo_domain: Option<String>,
    /// Sender from MAIL FROM
    mail_from: Option<String>,
    /// Accepted recipients from RCPT TO
    rcpt_to: Vec<String>,
    /// Whether STARTTLS has been negotiated
    _tls_active: bool,
}

pub async fn run(
    port: u16,
    maildir_path: String,
    domain: String,
    db: Arc<Database>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!(port, domain = %domain, "Inbound SMTP server listening");

    loop {
        let (stream, addr) = listener.accept().await?;
        info!(addr = %addr, "New inbound connection");

        let maildir_path = maildir_path.clone();
        let domain = domain.clone();
        let db = db.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, addr, maildir_path, domain, db).await {
                error!(addr = %addr, error = ?e, "Connection error");
            }
        });
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    maildir_path: String,
    domain: String,
    db: Arc<Database>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    let mut session = SmtpSession {
        domain: domain.clone(),
        maildir_path,
        db,
        helo_domain: None,
        mail_from: None,
        rcpt_to: Vec::new(),
        _tls_active: false,
    };

    // Greeting
    writer
        .write_all(b"220 Stampd ESMTP ready\r\n")
        .await?;

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // Connection closed
        }

        let cmd_line = line.trim_end_matches("\r\n").to_string();
        if cmd_line.is_empty() {
            continue;
        }

        info!(addr = %addr, cmd = %cmd_line, "Inbound SMTP");

        // Split into command verb and arguments
        let (verb, args) = match cmd_line.split_once(' ') {
            Some((v, a)) => (v.to_uppercase(), Some(a.to_string())),
            None => (cmd_line.to_uppercase(), None),
        };

        let response = match verb.as_str() {
            "HELO" | "EHLO" => {
                let client_domain = args.unwrap_or_default();
                session.helo_domain = Some(client_domain.clone());
                if verb == "EHLO" {
                    format!(
                        "250-{}\r\n250-8BITMIME\r\n250-STARTTLS\r\n250 SIZE 52428800\r\n",
                        domain
                    )
                } else {
                    format!("250 {}\r\n", domain)
                }
            }
            "STARTTLS" => {
                // TODO: Phase 0.2.3 — implement TLS upgrade
                "454 TLS not available\r\n".to_string()
            }
            "MAIL" => {
                match parse_mail_from(&args.unwrap_or_default()) {
                    Ok(sender) => {
                        session.mail_from = Some(sender.clone());
                        session.rcpt_to.clear(); // RSET-like on new MAIL
                        info!(addr = %addr, sender = %sender, "MAIL FROM accepted");
                        "250 OK\r\n".to_string()
                    }
                    Err(e) => {
                        warn!(addr = %addr, error = %e, "MAIL FROM rejected");
                        format!("501 {}\r\n", e)
                    }
                }
            }
            "RCPT" => {
                match parse_rcpt_to(&args.unwrap_or_default()) {
                    Ok(recipient) => {
                        // Validate domain matches our server
                        let recipient_addr = extract_address(&recipient);
                        let rcpt_domain = extract_domain(&recipient_addr);
                        if rcpt_domain != session.domain {
                            info!(
                                addr = %addr,
                                recipient = %recipient_addr,
                                rcpt_domain = %rcpt_domain,
                                expected = %session.domain,
                                "RCPT TO rejected — wrong domain"
                            );
                            "550 User not local, please try <forwarding address>\r\n".to_string()
                        } else {
                            session.rcpt_to.push(recipient_addr.clone());
                            info!(addr = %addr, recipient = %recipient_addr, "RCPT TO accepted");
                            "250 OK\r\n".to_string()
                        }
                    }
                    Err(e) => {
                        warn!(addr = %addr, error = %e, "RCPT TO rejected");
                        format!("501 {}\r\n", e)
                    }
                }
            }
            "DATA" => {
                if session.mail_from.is_none() || session.rcpt_to.is_empty() {
                    "503 Bad sequence of commands (need MAIL FROM and RCPT TO first)\r\n".to_string()
                } else {
                    // Accept DATA and read message body
                    writer.write_all(b"354 Start mail input; end with <CRLF>.<CRLF>\r\n").await?;

                    let message = read_message_body(&mut reader, &mut line).await;

                    // Store in Maildir for each recipient
                    match store_message(&session, &message).await {
                        Ok(count) => {
                            info!(
                                addr = %addr,
                                recipients = count,
                                size = message.len(),
                                "Message accepted and stored"
                            );
                            // Reset session for next message
                            session.mail_from = None;
                            session.rcpt_to.clear();
                            "250 OK: Message accepted for delivery\r\n".to_string()
                        }
                        Err(e) => {
                            error!(addr = %addr, error = ?e, "Failed to store message");
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

    info!(addr = %addr, "Inbound connection closed");
    Ok(())
}

// ── Command Parsers ──────────────────────────────────────────────

/// Parse MAIL FROM:<sender> — returns the sender address (or empty for bounce).
fn parse_mail_from(args: &str) -> Result<String, String> {
    let args = args.trim();
    // Expected: MAIL FROM:<addr> or MAIL FROM:<> (bounce)
    if let Some(start) = args.find('<') {
        if let Some(end) = args.find('>') {
            let addr = &args[start + 1..end];
            // Remove any SIZE= parameter that might follow
            let addr = addr.split_whitespace().next().unwrap_or(addr);
            Ok(addr.to_string())
        } else {
            Err("Missing '>' in MAIL FROM".to_string())
        }
    } else {
        Err("MAIL FROM requires <address> syntax".to_string())
    }
}

/// Parse RCPT TO:<recipient> — returns the full address.
fn parse_rcpt_to(args: &str) -> Result<String, String> {
    let args = args.trim();
    if let Some(start) = args.find('<') {
        if let Some(end) = args.find('>') {
            let addr = &args[start + 1..end];
            if addr.is_empty() {
                return Err("RCPT TO requires an address".to_string());
            }
            Ok(addr.to_string())
        } else {
            Err("Missing '>' in RCPT TO".to_string())
        }
    } else {
        Err("RCPT TO requires <address> syntax".to_string())
    }
}

/// Extract the bare email address from something like "user@domain" or "<user@domain>".
fn extract_address(input: &str) -> String {
    let s = input.trim();
    if let Some(start) = s.find('<') {
        if let Some(end) = s.find('>') {
            return s[start + 1..end].to_string();
        }
    }
    s.to_string()
}

/// Extract the domain from an email address.
fn extract_domain(addr: &str) -> String {
    addr.split('@')
        .nth(1)
        .unwrap_or("")
        .to_lowercase()
}

// ── DATA Body Reading ────────────────────────────────────────────

/// Read the message body until the dot-stuffing terminator (<CRLF>.<CRLF>).
async fn read_message_body(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
    line: &mut String,
) -> Vec<u8> {
    let mut body = Vec::new();

    loop {
        line.clear();
        if reader.read_line(line).await.unwrap_or(0) == 0 {
            break; // Connection closed prematurely
        }

        let raw_line = line.as_bytes();
        let trimmed = line.trim_end_matches("\r\n");

        // Check for the terminating line: "."
        if trimmed == "." {
            break;
        }

        // Handle dot-stuffing: ".." at start of line means a single "."
        if trimmed.starts_with("..") {
            body.extend_from_slice(&raw_line[1..]); // Remove one leading dot
        } else {
            body.extend_from_slice(raw_line);
        }
    }

    body
}

// ── Maildir Storage ──────────────────────────────────────────────

/// Store the message to Maildir for each recipient.
async fn store_message(session: &SmtpSession, message: &[u8]) -> anyhow::Result<usize> {
    let mut count = 0;

    // Parse the message to extract From/To/Subject for the Maildir filename
    let message_id = format!(
        "{}.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
        std::process::id()
    );

    // Build the full .eml content with envelope info
    let _sender = session.mail_from.as_deref().unwrap_or("");

    for recipient in &session.rcpt_to {
        let local_part = recipient.split('@').next().unwrap_or("unknown");

        // Create Maildir path for this user
        let user_dir = std::path::Path::new(&session.maildir_path)
            .join(&session.domain)
            .join(local_part);

        // Ensure directories exist
        tokio::fs::create_dir_all(user_dir.join("cur")).await?;
        tokio::fs::create_dir_all(user_dir.join("new")).await?;
        tokio::fs::create_dir_all(user_dir.join("tmp")).await?;

        // Generate Maildir filename: timestamp.pid.hostname:2,info
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "localhost".to_string());

        let filename = format!(
            "{}.{}.{}:2,",
            message_id, std::process::id(), hostname
        );

        let filepath = user_dir.join("new").join(&filename);

        // Write the message
        tokio::fs::write(&filepath, message).await?;
        info!(
            recipient = %recipient,
            path = %filepath.display(),
            "Stored message in Maildir"
        );
        count += 1;
    }

    Ok(count)
}
