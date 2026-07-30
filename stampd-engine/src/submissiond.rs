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
use crate::stats::EngineStats;

/// Per-session state for submission.
struct SubmissionSession {
    db: Arc<Database>,
    maildir_path: String,
    domain: String,
    dkim_selector: String,
    dkim_signer: Option<DkimSigner>,
    tls_config: Option<Arc<rustls::ServerConfig>>,
    /// Authenticated user id (None until AUTH succeeds)
    authenticated_user_id: Option<i64>,
    /// Sender from MAIL FROM
    mail_from: Option<String>,
    /// Recipients
    rcpt_to: Vec<String>,
    /// TLS active flag
    tls_active: bool,
    /// AUTH LOGIN state machine
    auth_state: AuthState,
    /// Pending username during AUTH LOGIN flow
    pending_username: Option<String>,
}

/// Multi-step AUTH LOGIN state.
enum AuthState {
    /// Not in an AUTH LOGIN flow.
    None,
    /// Waiting for base64-encoded username.
    AwaitingUsername,
    /// Waiting for base64-encoded password.
    AwaitingPassword,
}

pub async fn run(
    port: u16,
    dkim_selector: String,
    db: Arc<Database>,
    dkim_signer: Option<DkimSigner>,
    tls_config: Option<Arc<rustls::ServerConfig>>,
    stats: Arc<EngineStats>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!(port, dkim = dkim_signer.is_some(), tls = tls_config.is_some(), "Submission server listening");

    // Get domain from server_config
    let (domain, _, _) = db.get_server_config()
        .unwrap_or(("localhost".to_string(), true, "default".to_string()));

    loop {
        let (stream, addr) = listener.accept().await?;
        info!(addr = %addr, "New submission connection");
        stats.connection_opened();

        let db = db.clone();
        let dkim_selector = dkim_selector.clone();
        let domain = domain.clone();
        let dkim_signer = dkim_signer.clone();
        let tls_config = tls_config.clone();
        let stats = stats.clone();

        tokio::spawn(async move {
            let result = handle_submission(stream, addr, db, dkim_selector, domain, dkim_signer, tls_config).await;
            stats.connection_closed();
            if let Err(e) = result {
                error!(addr = %addr, error = ?e, "Submission error");
            }
        });
    }
}

async fn handle_submission(
    mut stream: tokio::net::TcpStream,
    addr: SocketAddr,
    db: Arc<Database>,
    dkim_selector: String,
    domain: String,
    dkim_signer: Option<DkimSigner>,
    tls_config: Option<Arc<rustls::ServerConfig>>,
) -> anyhow::Result<()> {
    let mut line = String::new();

    let mut session = SubmissionSession {
        db,
        maildir_path: "/var/lib/stampd/mail".to_string(), // TODO: pass from config
        domain: domain.clone(),
        dkim_selector,
        dkim_signer,
        tls_config,
        authenticated_user_id: None,
        mail_from: None,
        rcpt_to: Vec::new(),
        tls_active: false,
        auth_state: AuthState::None,
        pending_username: None,
    };

    // Greeting
    stream
        .write_all(b"220 Stampd Submission ready\r\n")
        .await?;

    loop {
        line.clear();
        // Read line from current stream (plain or TLS)
        let bytes_read = {
            let mut buf_reader = BufReader::new(&mut stream);
            buf_reader.read_line(&mut line).await?
        };

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
                    let mut ehlo = format!(
                        "250-{}\r\n250-8BITMIME\r\n250-AUTH PLAIN LOGIN\r\n",
                        domain
                    );
                    // Only advertise STARTTLS if TLS is configured and not yet active
                    if session.tls_config.is_some() && !session.tls_active {
                        ehlo.push_str("250-STARTTLS\r\n");
                    }
                    ehlo.push_str("250 SIZE 52428800\r\n");
                    ehlo
                } else {
                    format!("250 {}\r\n", domain)
                }
            }
            "AUTH" => {
                match &session.auth_state {
                    AuthState::AwaitingUsername => {
                        // This is the username response to 334 VXNlcm5hbWU6
                        let args = args.unwrap_or_default();
                        match base64_decode(&args) {
                            Some(bytes) => {
                                let username = String::from_utf8_lossy(&bytes).to_string();
                                session.pending_username = Some(username);
                                session.auth_state = AuthState::AwaitingPassword;
                                "334 UGFzc3dvcmQ6\r\n".to_string()
                            }
                            None => {
                                session.auth_state = AuthState::None;
                                session.pending_username = None;
                                "501 Invalid encoding\r\n".to_string()
                            }
                        }
                    }
                    AuthState::AwaitingPassword => {
                        // This is the password response to 334 UGFzc3dvcmQ6
                        let args = args.unwrap_or_default();
                        match base64_decode(&args) {
                            Some(password_bytes) => {
                                let password = String::from_utf8_lossy(&password_bytes);
                                let username = session.pending_username.take().unwrap_or_default();
                                session.auth_state = AuthState::None;
                                match authenticate_user(&username, &password, &session.db) {
                                    Ok(user_id) => {
                                        session.authenticated_user_id = Some(user_id);
                                        info!(addr = %addr, user_id, "AUTH LOGIN successful");
                                        "235 Authentication successful\r\n".to_string()
                                    }
                                    Err(e) => {
                                        warn!(addr = %addr, error = %e, "AUTH LOGIN failed");
                                        format!("535 {}\r\n", e)
                                    }
                                }
                            }
                            None => {
                                session.auth_state = AuthState::None;
                                session.pending_username = None;
                                "501 Invalid encoding\r\n".to_string()
                            }
                        }
                    }
                    AuthState::None => {
                        // Start of AUTH command
                        let args = args.unwrap_or_default();
                        if args.starts_with("PLAIN ") {
                            // AUTH PLAIN inline
                            let encoded = &args[6..];
                            match base64_decode(encoded) {
                                Some(decoded) => {
                                    let parts: Vec<&[u8]> = decoded.split(|&b| b == 0).collect();
                                    if parts.len() < 3 {
                                        "501 Invalid AUTH PLAIN format\r\n".to_string()
                                    } else {
                                        let username = String::from_utf8_lossy(parts[1]);
                                        let password = String::from_utf8_lossy(parts[2]);
                                        match authenticate_user(&username, &password, &session.db) {
                                            Ok(user_id) => {
                                                session.authenticated_user_id = Some(user_id);
                                                info!(addr = %addr, user_id, "AUTH PLAIN successful");
                                                "235 Authentication successful\r\n".to_string()
                                            }
                                            Err(e) => {
                                                warn!(addr = %addr, error = %e, "AUTH PLAIN failed");
                                                format!("535 {}\r\n", e)
                                            }
                                        }
                                    }
                                }
                                None => "501 Invalid base64 encoding\r\n".to_string(),
                            }
                        } else if args.starts_with("LOGIN") {
                            let login_args = args.trim_start_matches("LOGIN").trim();
                            if !login_args.is_empty() {
                                // AUTH LOGIN with inline username
                                match base64_decode(login_args) {
                                    Some(bytes) => {
                                        let username = String::from_utf8_lossy(&bytes).to_string();
                                        session.pending_username = Some(username);
                                        session.auth_state = AuthState::AwaitingPassword;
                                        "334 UGFzc3dvcmQ6\r\n".to_string()
                                    }
                                    None => "501 Invalid encoding\r\n".to_string(),
                                }
                            } else {
                                // AUTH LOGIN without inline username
                                session.auth_state = AuthState::AwaitingUsername;
                                "334 VXNlcm5hbWU6\r\n".to_string()
                            }
                        } else {
                            "501 Unsupported AUTH mechanism\r\n".to_string()
                        }
                    }
                }
            }
            "STARTTLS" => {
                if session.tls_active {
                    "503 TLS already active\r\n".to_string()
                } else if let Some(tls_cfg) = session.tls_config.clone() {
                    // Acknowledge STARTTLS
                    stream.write_all(b"220 Ready to start TLS\r\n").await?;

                    // Perform TLS handshake on the raw stream
                    let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_cfg);
                    match tls_acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            info!(addr = %addr, "STARTTLS handshake successful");
                            session.tls_active = true;

                            // Continue session over TLS
                            tls_session(tls_stream, &mut session, &mut line, &addr).await?;
                            return Ok(());
                        }
                        Err(e) => {
                            warn!(addr = %addr, error = ?e, "STARTTLS handshake failed");
                            return Ok(());
                        }
                    }
                } else {
                    "454 TLS not available\r\n".to_string()
                }
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
                    stream.write_all(b"354 Start mail input; end with <CRLF>.<CRLF>\r\n").await?;

                    let message = read_message_body(&mut stream, &mut line).await;

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
                stream.write_all(b"221 Bye\r\n").await?;
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

        stream.write_all(response.as_bytes()).await?;
    }

    info!(addr = %addr, "Submission connection closed");
    Ok(())
}

/// Handle SMTP submission session over TLS stream.
async fn tls_session(
    mut tls_stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    session: &mut SubmissionSession,
    line: &mut String,
    addr: &SocketAddr,
) -> anyhow::Result<()> {
    loop {
        line.clear();
        let bytes_read = {
            let mut buf_reader = BufReader::new(&mut tls_stream);
            buf_reader.read_line(line).await?
        };

        if bytes_read == 0 {
            break;
        }

        let cmd_line = line.trim_end_matches("\r\n").to_string();
        if cmd_line.is_empty() {
            continue;
        }

        info!(addr = %addr, cmd = %cmd_line, tls = true, "Submission (TLS)");

        let (verb, args) = match cmd_line.split_once(' ') {
            Some((v, a)) => (v.to_uppercase(), Some(a.to_string())),
            None => (cmd_line.to_uppercase(), None),
        };

        let response = match verb.as_str() {
            "HELO" | "EHLO" => {
                if verb == "EHLO" {
                    format!(
                        "250-{}\r\n250-8BITMIME\r\n250-AUTH PLAIN LOGIN\r\n250 SIZE 52428800\r\n",
                        session.domain
                    )
                } else {
                    format!("250 {}\r\n", session.domain)
                }
            }
            "AUTH" => {
                // AUTH is not allowed after STARTTLS in this implementation
                // (TLS session inherits the authenticated state from before)
                if session.authenticated_user_id.is_some() {
                    "235 Already authenticated\r\n".to_string()
                } else {
                    "530 Authentication required before STARTTLS\r\n".to_string()
                }
            }
            "MAIL" => {
                if session.authenticated_user_id.is_none() {
                    "530 Authentication required\r\n".to_string()
                } else {
                    match parse_address(&args.unwrap_or_default()) {
                        Ok(sender) => {
                            session.mail_from = Some(sender.clone());
                            session.rcpt_to.clear();
                            info!(addr = %addr, sender = %sender, "MAIL FROM accepted (TLS)");
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
                                info!(addr = %addr, recipient = %recipient, "RCPT TO accepted (TLS)");
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
                    tls_stream.write_all(b"354 Start mail input; end with <CRLF>.<CRLF>\r\n").await?;

                    let message = read_message_body(&mut tls_stream, line).await;

                    // Store the message to a temp file and enqueue for each recipient
                    match enqueue_message(session, &message).await {
                        Ok(count) => {
                            info!(
                                addr = %addr,
                                recipients = count,
                                "Message enqueued for delivery (TLS)"
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
                tls_stream.write_all(b"221 Bye\r\n").await?;
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

        tls_stream.write_all(response.as_bytes()).await?;
    }

    info!(addr = %addr, "Submission TLS session closed");
    Ok(())
}

// ── Authentication ────────────────────────────────────────────────

/// Authenticate a user by email and password.
fn authenticate_user(email: &str, password: &str, db: &Database) -> Result<i64, String> {
    let (user_id, password_hash, _is_admin, disabled) = db.get_user_by_email(email)
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or("Invalid credentials".to_string())?;

    if disabled {
        return Err("Account is disabled".to_string());
    }

    // Verify password with argon2id
    if verify_password(password, &password_hash) {
        Ok(user_id)
    } else {
        Err("Invalid credentials".to_string())
    }
}

/// Verify a password against its hash using argon2id.
fn verify_password(password: &str, hash: &str) -> bool {
    use argon2::{Argon2, password_hash::{PasswordHash, PasswordVerifier}};

    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
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
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    line: &mut String,
) -> Vec<u8> {
    let mut body = Vec::new();
    let mut buf_reader = BufReader::new(reader);

    loop {
        line.clear();
        if buf_reader.read_line(line).await.unwrap_or(0) == 0 {
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
