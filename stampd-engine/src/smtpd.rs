//! Inbound SMTP server (port 25).
//!
//! RFC 5321 command handling: HELO/EHLO, MAIL FROM, RCPT TO, DATA, RSET, QUIT.
//! STARTTLS (RFC 3207) — plaintext initially, upgrade on STARTTLS command.
//! No auth required for inbound (standard internet behavior).
//! Rejects any RCPT TO not addressed to the server's configured domain.
//! Best-effort SPF check on sender IP.

use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, AsyncRead};
use tracing::{info, warn, error};
use std::sync::Arc;
use std::net::SocketAddr;

use crate::db::Database;
use crate::tls::TlsConfig;
use crate::spf::check_spf;
use crate::filters::{self, FilterContext, HookPoint};

/// Per-session state during an SMTP transaction.
struct SmtpSession {
    domain: String,
    maildir_path: String,
    db: Arc<Database>,
    tls_config: Option<Arc<rustls::ServerConfig>>,
    filters_dir: std::path::PathBuf,
    filters_timeout_ms: u64,
    gateway_url: Option<String>,
    helo_domain: Option<String>,
    mail_from: Option<String>,
    rcpt_to: Vec<String>,
    sender_ip: std::net::IpAddr,
    tls_active: bool,
}

pub async fn run(
    port: u16,
    maildir_path: String,
    domain: String,
    db: Arc<Database>,
    tls_config: Option<TlsConfig>,
    filters_dir: std::path::PathBuf,
    filters_timeout_ms: u64,
    gateway_url: Option<String>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!(port, domain = %domain, tls = tls_config.is_some(), "Inbound SMTP server listening");

    let tls_config = tls_config.map(|tc| tc.server_config);

    loop {
        let (stream, addr) = listener.accept().await?;
        info!(addr = %addr, "New inbound connection");

        let maildir_path = maildir_path.clone();
        let domain = domain.clone();
        let db = db.clone();
        let tls_config = tls_config.clone();
        let filters_dir = filters_dir.clone();
        let gateway_url = gateway_url.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, addr, maildir_path, domain, db, tls_config, filters_dir, filters_timeout_ms, gateway_url).await {
                error!(addr = %addr, error = ?e, "Connection error");
            }
        });
    }
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    addr: SocketAddr,
    maildir_path: String,
    domain: String,
    db: Arc<Database>,
    tls_config: Option<Arc<rustls::ServerConfig>>,
    filters_dir: std::path::PathBuf,
    filters_timeout_ms: u64,
    gateway_url: Option<String>,
) -> anyhow::Result<()> {
    let sender_ip = stream.peer_addr()?.ip();
    let mut line = String::new();

    let mut session = SmtpSession {
        domain: domain.clone(),
        maildir_path,
        db,
        tls_config,
        filters_dir,
        filters_timeout_ms,
        gateway_url,
        helo_domain: None,
        mail_from: None,
        rcpt_to: Vec::new(),
        sender_ip,
        tls_active: false,
    };

    // Greeting
    stream.write_all(b"220 Stampd ESMTP ready\r\n").await?;

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

        info!(addr = %addr, cmd = %cmd_line, tls = session.tls_active, "Inbound SMTP");

        let (verb, args) = match cmd_line.split_once(' ') {
            Some((v, a)) => (v.to_uppercase(), Some(a.to_string())),
            None => (cmd_line.to_uppercase(), None),
        };

        let response = match verb.as_str() {
            "HELO" | "EHLO" => {
                let client_domain = args.unwrap_or_default();
                session.helo_domain = Some(client_domain.clone());
                if verb == "EHLO" {
                    if session.tls_config.is_some() && !session.tls_active {
                        format!(
                            "250-{}\r\n250-8BITMIME\r\n250-STARTTLS\r\n250 SIZE 52428800\r\n",
                            domain
                        )
                    } else {
                        format!(
                            "250-{}\r\n250-8BITMIME\r\n250 SIZE 52428800\r\n",
                            domain
                        )
                    }
                } else {
                    format!("250 {}\r\n", domain)
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
                match parse_mail_from(&args.unwrap_or_default()) {
                    Ok(sender) => {
                        // Run mail_from filter hooks
                        let ctx = FilterContext {
                            hook: HookPoint::MailFrom,
                            sender: sender.clone(),
                            recipient: String::new(),
                            helo_domain: session.helo_domain.clone().unwrap_or_default(),
                            client_ip: session.sender_ip.to_string(),
                            tls: session.tls_active,
                            headers: None,
                            body: None,
                        };
                        match filters::run_filters(&session.filters_dir, HookPoint::MailFrom, &ctx, session.filters_timeout_ms, session.gateway_url.as_deref()).await {
                            Ok(()) => {
                                session.mail_from = Some(sender.clone());
                                session.rcpt_to.clear();
                                info!(addr = %addr, sender = %sender, "MAIL FROM accepted");
                                "250 OK\r\n".to_string()
                            }
                            Err(reason) => {
                                warn!(addr = %addr, sender = %sender, reason = %reason, "MAIL FROM rejected by filter");
                                format!("550 {}\r\n", reason)
                            }
                        }
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
                        let recipient_addr = extract_address(&recipient);
                        let rcpt_domain = extract_domain(&recipient_addr);
                        // Check configured domain OR verified custom domains
                        if !session.db.is_domain_allowed(&rcpt_domain) {
                            info!(
                                addr = %addr,
                                recipient = %recipient_addr,
                                rcpt_domain = %rcpt_domain,
                                "RCPT TO rejected — domain not configured"
                            );
                            "550 User not local, please try <forwarding address>\r\n".to_string()
                        } else {
                            // Run rcpt_to filter hooks
                            let ctx = FilterContext {
                                hook: HookPoint::RcptTo,
                                sender: session.mail_from.clone().unwrap_or_default(),
                                recipient: recipient_addr.clone(),
                                helo_domain: session.helo_domain.clone().unwrap_or_default(),
                                client_ip: session.sender_ip.to_string(),
                                tls: session.tls_active,
                                headers: None,
                                body: None,
                            };
                            match filters::run_filters(&session.filters_dir, HookPoint::RcptTo, &ctx, session.filters_timeout_ms, session.gateway_url.as_deref()).await {
                                Ok(()) => {
                                    session.rcpt_to.push(recipient_addr.clone());
                                    info!(addr = %addr, recipient = %recipient_addr, "RCPT TO accepted");
                                    "250 OK\r\n".to_string()
                                }
                                Err(reason) => {
                                    warn!(addr = %addr, recipient = %recipient_addr, reason = %reason, "RCPT TO rejected by filter");
                                    format!("550 {}\r\n", reason)
                                }
                            }
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
                    // SPF check
                    let sender_domain = extract_domain(session.mail_from.as_deref().unwrap_or(""));
                    let spf_result = check_spf(&sender_domain, session.sender_ip).await;
                    info!(
                        addr = %addr,
                        sender_domain = %sender_domain,
                        passed = spf_result.passed,
                        message = %spf_result.message,
                        "SPF check result"
                    );

                    stream.write_all(b"354 Start mail input; end with <CRLF>.<CRLF>\r\n").await?;

                    let message = read_message_body(&mut stream, &mut line).await;

                    // Run data filter hooks (pass full message)
                    let msg_str = String::from_utf8_lossy(&message).to_string();
                    let (headers, body) = match msg_str.find("\r\n\r\n") {
                        Some(pos) => (Some(msg_str[..pos].to_string()), Some(msg_str[pos + 4..].to_string())),
                        None => (Some(msg_str.clone()), None),
                    };
                    let ctx = FilterContext {
                        hook: HookPoint::Data,
                        sender: session.mail_from.clone().unwrap_or_default(),
                        recipient: session.rcpt_to.join(", "),
                        helo_domain: session.helo_domain.clone().unwrap_or_default(),
                        client_ip: session.sender_ip.to_string(),
                        tls: session.tls_active,
                        headers,
                        body,
                    };
                    if let Err(reason) = filters::run_filters(&session.filters_dir, HookPoint::Data, &ctx, session.filters_timeout_ms, session.gateway_url.as_deref()).await {
                        warn!(addr = %addr, reason = %reason, "Message rejected by DATA filter");
                        session.mail_from = None;
                        session.rcpt_to.clear();
                        format!("550 {}\r\n", reason)
                    } else {
                        match store_message(&session, &message).await {
                            Ok(count) => {
                                info!(
                                    addr = %addr,
                                    recipients = count,
                                    size = message.len(),
                                    "Message accepted and stored"
                                );
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

    info!(addr = %addr, "Inbound connection closed");
    Ok(())
}

/// Handle SMTP session over TLS stream.
async fn tls_session(
    mut tls_stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    session: &mut SmtpSession,
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

        info!(addr = %addr, cmd = %cmd_line, tls = true, "Inbound SMTP (TLS)");

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
                        "250-{}\r\n250-8BITMIME\r\n250 SIZE 52428800\r\n",
                        session.domain
                    )
                } else {
                    format!("250 {}\r\n", session.domain)
                }
            }
            "MAIL" => {
                match parse_mail_from(&args.unwrap_or_default()) {
                    Ok(sender) => {
                        // Run mail_from filter hooks
                        let ctx = FilterContext {
                            hook: HookPoint::MailFrom,
                            sender: sender.clone(),
                            recipient: String::new(),
                            helo_domain: session.helo_domain.clone().unwrap_or_default(),
                            client_ip: session.sender_ip.to_string(),
                            tls: session.tls_active,
                            headers: None,
                            body: None,
                        };
                        match filters::run_filters(&session.filters_dir, HookPoint::MailFrom, &ctx, session.filters_timeout_ms, session.gateway_url.as_deref()).await {
                            Ok(()) => {
                                session.mail_from = Some(sender.clone());
                                session.rcpt_to.clear();
                                info!(addr = %addr, sender = %sender, "MAIL FROM accepted (TLS)");
                                "250 OK\r\n".to_string()
                            }
                            Err(reason) => {
                                warn!(addr = %addr, sender = %sender, reason = %reason, "MAIL FROM rejected by filter (TLS)");
                                format!("550 {}\r\n", reason)
                            }
                        }
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
                        let recipient_addr = extract_address(&recipient);
                        let rcpt_domain = extract_domain(&recipient_addr);
                        // Check configured domain OR verified custom domains
                        if !session.db.is_domain_allowed(&rcpt_domain) {
                            info!(
                                addr = %addr,
                                recipient = %recipient_addr,
                                rcpt_domain = %rcpt_domain,
                                "RCPT TO rejected — domain not configured"
                            );
                            "550 User not local, please try <forwarding address>\r\n".to_string()
                        } else {
                            // Run rcpt_to filter hooks
                            let ctx = FilterContext {
                                hook: HookPoint::RcptTo,
                                sender: session.mail_from.clone().unwrap_or_default(),
                                recipient: recipient_addr.clone(),
                                helo_domain: session.helo_domain.clone().unwrap_or_default(),
                                client_ip: session.sender_ip.to_string(),
                                tls: session.tls_active,
                                headers: None,
                                body: None,
                            };
                            match filters::run_filters(&session.filters_dir, HookPoint::RcptTo, &ctx, session.filters_timeout_ms, session.gateway_url.as_deref()).await {
                                Ok(()) => {
                                    session.rcpt_to.push(recipient_addr.clone());
                                    info!(addr = %addr, recipient = %recipient_addr, "RCPT TO accepted (TLS)");
                                    "250 OK\r\n".to_string()
                                }
                                Err(reason) => {
                                    warn!(addr = %addr, recipient = %recipient_addr, reason = %reason, "RCPT TO rejected by filter (TLS)");
                                    format!("550 {}\r\n", reason)
                                }
                            }
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
                    let sender_domain = extract_domain(session.mail_from.as_deref().unwrap_or(""));
                    let spf_result = check_spf(&sender_domain, session.sender_ip).await;
                    info!(
                        addr = %addr,
                        sender_domain = %sender_domain,
                        passed = spf_result.passed,
                        message = %spf_result.message,
                        "SPF check result (TLS)"
                    );

                    tls_stream.write_all(b"354 Start mail input; end with <CRLF>.<CRLF>\r\n").await?;

                    let message = read_message_body(&mut tls_stream, line).await;

                    // Run data filter hooks (pass full message)
                    let msg_str = String::from_utf8_lossy(&message).to_string();
                    let (headers, body) = match msg_str.find("\r\n\r\n") {
                        Some(pos) => (Some(msg_str[..pos].to_string()), Some(msg_str[pos + 4..].to_string())),
                        None => (Some(msg_str.clone()), None),
                    };
                    let ctx = FilterContext {
                        hook: HookPoint::Data,
                        sender: session.mail_from.clone().unwrap_or_default(),
                        recipient: session.rcpt_to.join(", "),
                        helo_domain: session.helo_domain.clone().unwrap_or_default(),
                        client_ip: session.sender_ip.to_string(),
                        tls: session.tls_active,
                        headers,
                        body,
                    };
                    if let Err(reason) = filters::run_filters(&session.filters_dir, HookPoint::Data, &ctx, session.filters_timeout_ms, session.gateway_url.as_deref()).await {
                        warn!(addr = %addr, reason = %reason, "Message rejected by DATA filter (TLS)");
                        session.mail_from = None;
                        session.rcpt_to.clear();
                        format!("550 {}\r\n", reason)
                    } else {
                        match store_message(session, &message).await {
                            Ok(count) => {
                                info!(
                                    addr = %addr,
                                    recipients = count,
                                    size = message.len(),
                                    "Message accepted and stored (TLS)"
                                );
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

    info!(addr = %addr, "TLS session closed");
    Ok(())
}

// ── Command Parsers ──────────────────────────────────────────────

fn parse_mail_from(args: &str) -> Result<String, String> {
    let args = args.trim();
    if let Some(start) = args.find('<') {
        if let Some(end) = args.find('>') {
            let addr = &args[start + 1..end];
            let addr = addr.split_whitespace().next().unwrap_or(addr);
            Ok(addr.to_string())
        } else {
            Err("Missing '>' in MAIL FROM".to_string())
        }
    } else {
        Err("MAIL FROM requires <address> syntax".to_string())
    }
}

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

fn extract_address(input: &str) -> String {
    let s = input.trim();
    if let Some(start) = s.find('<') {
        if let Some(end) = s.find('>') {
            return s[start + 1..end].to_string();
        }
    }
    s.to_string()
}

fn extract_domain(addr: &str) -> String {
    addr.split('@')
        .nth(1)
        .unwrap_or("")
        .to_lowercase()
}

// ── DATA Body Reading ────────────────────────────────────────────

async fn read_message_body(
    reader: &mut (impl AsyncRead + Unpin),
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

// ── Maildir Storage ──────────────────────────────────────────────

async fn store_message(session: &SmtpSession, message: &[u8]) -> anyhow::Result<usize> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut count = 0;

    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let message_id = format!(
        "{}.{:05}.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
        seq % 100000,
        std::process::id()
    );

    for recipient in &session.rcpt_to {
        let local_part = recipient.split('@').next().unwrap_or("unknown");
        let rcpt_domain = recipient.split('@').nth(1).unwrap_or("");

        // For custom domains, route to the domain owner's mailbox
        let mailbox_user = session.db.get_mailbox_user_for_domain(rcpt_domain, recipient)
            .unwrap_or_else(|| local_part.to_string());

        let user_dir = std::path::Path::new(&session.maildir_path)
            .join(&session.domain)
            .join(&mailbox_user);

        for subdir in &["cur", "new", "tmp", "sent", "drafts", "archive", "spam"] {
            tokio::fs::create_dir_all(user_dir.join(subdir)).await?;
        }

        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "localhost".to_string());

        let filename = format!(
            "{}.{}.{}:2,",
            message_id, std::process::id(), hostname
        );

        let filepath = user_dir.join("new").join(&filename);

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
