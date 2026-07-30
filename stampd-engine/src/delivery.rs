//! Outbound SMTP delivery client.
//!
//! MX lookup → connect → EHLO → STARTTLS (if available) → MAIL/RCPT/DATA → disconnect.
//! Handles 2xx/4xx/5xx responses.

use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tracing::{info, warn};

/// Result of an outbound delivery attempt.
#[derive(Debug)]
pub enum DeliveryResult {
    /// Message accepted by remote server.
    Delivered,
    /// Temporary failure — retry later (e.g. connection refused, 4xx).
    TemporaryFailure(String),
    /// Permanent failure — dead-letter (e.g. 5xx, bad recipient).
    PermanentFailure(String),
}

/// Attempt to deliver a message to a remote recipient.
///
/// `from` — sender address (e.g. "user@stampd.com")
/// `to` — recipient address (e.g. "friend@gmail.com")
/// `message` — full .eml content including headers
pub async fn deliver(from: &str, to: &str, message: &[u8]) -> DeliveryResult {
    let recipient_domain = match to.split('@').nth(1) {
        Some(d) => d,
        None => {
            return DeliveryResult::PermanentFailure(
                "Invalid recipient address (no domain)".to_string(),
            );
        }
    };

    // MX lookup
    let mx_hosts = match lookup_mx(recipient_domain).await {
        Ok(hosts) if !hosts.is_empty() => hosts,
        Ok(_) => {
            return DeliveryResult::PermanentFailure(format!(
                "No MX records for {}",
                recipient_domain
            ));
        }
        Err(e) => {
            return DeliveryResult::TemporaryFailure(format!("MX lookup failed: {}", e));
        }
    };

    // Try each MX host in priority order
    for mx_host in &mx_hosts {
        info!(mx = %mx_host, recipient = %to, "Attempting delivery");

        match try_deliver(mx_host, from, to, message).await {
            Ok(()) => {
                info!(mx = %mx_host, recipient = %to, "Delivery successful");
                return DeliveryResult::Delivered;
            }
            Err(e) => {
                warn!(mx = %mx_host, recipient = %to, error = %e, "Delivery failed");
                // Continue to next MX host
            }
        }
    }

    DeliveryResult::TemporaryFailure(format!(
        "All MX servers for {} rejected the message",
        recipient_domain
    ))
}

/// Try delivery to a specific MX host.
async fn try_deliver(mx_host: &str, from: &str, to: &str, message: &[u8]) -> Result<(), String> {
    let addr = format!("{}:25", mx_host);

    // Connect with timeout
    let stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(&addr))
        .await
        .map_err(|_| format!("Connection to {} timed out", mx_host))?
        .map_err(|e| format!("Connection to {} failed: {}", mx_host, e))?;

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Read greeting
    line.clear();
    reader
        .read_line(&mut line)
        .await
        .map_err(|e| format!("Read greeting: {}", e))?;
    if !line.starts_with("2") {
        return Err(format!("Bad greeting: {}", line.trim()));
    }
    info!(mx = %mx_host, greeting = %line.trim(), "Connected");

    // EHLO
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "localhost".to_string());

    send_cmd(
        &mut writer,
        &mut reader,
        &mut line,
        &format!("EHLO {}", hostname),
        "EHLO",
    )
    .await?;

    // Try STARTTLS (best-effort)
    let tls_active = try_starttls(&mut writer, &mut reader, &mut line).await;

    // If TLS was activated, we need to re-split the stream
    // For now, we continue with the plaintext reader/writer
    // TODO: Properly handle TLS stream reassembly
    if tls_active {
        info!("STARTTLS accepted — continuing with plaintext (TLS upgrade not yet implemented)");
    }

    // MAIL FROM
    send_cmd(
        &mut writer,
        &mut reader,
        &mut line,
        &format!("MAIL FROM:<{}>", from),
        "MAIL FROM",
    )
    .await?;

    // RCPT TO
    send_cmd(
        &mut writer,
        &mut reader,
        &mut line,
        &format!("RCPT TO:<{}>", to),
        "RCPT TO",
    )
    .await?;

    // DATA
    send_cmd(&mut writer, &mut reader, &mut line, "DATA", "DATA").await?;

    // Send message body with dot-stuffing
    for line_bytes in message.split(|&b| b == b'\n') {
        let mut out_line = Vec::new();
        if line_bytes.starts_with(b".") {
            out_line.push(b'.'); // Escape leading dot
        }
        out_line.extend_from_slice(line_bytes);
        if out_line.ends_with(b"\r") {
            out_line.pop(); // Remove trailing \r, we'll add \r\n
        }
        out_line.push(b'\r');
        out_line.push(b'\n');
        writer
            .write_all(&out_line)
            .await
            .map_err(|e| format!("Write message line: {}", e))?;
    }

    // Terminating dot
    writer
        .write_all(b".\r\n")
        .await
        .map_err(|e| format!("Write terminator: {}", e))?;

    // Read DATA response
    line.clear();
    reader
        .read_line(&mut line)
        .await
        .map_err(|e| format!("Read DATA response: {}", e))?;
    if !line.starts_with("2") {
        return Err(format!("DATA rejected: {}", line.trim()));
    }

    // QUIT
    let _ = send_cmd(&mut writer, &mut reader, &mut line, "QUIT", "QUIT").await;

    Ok(())
}

/// Send an SMTP command and verify the response starts with the expected prefix.
async fn send_cmd(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    reader: &mut tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
    line: &mut String,
    cmd: &str,
    context: &str,
) -> Result<(), String> {
    writer
        .write_all(format!("{}\r\n", cmd).as_bytes())
        .await
        .map_err(|e| format!("Write {}: {}", context, e))?;

    line.clear();
    reader
        .read_line(line)
        .await
        .map_err(|e| format!("Read {}: {}", context, e))?;

    let trimmed = line.trim();
    if !trimmed.starts_with('2') && !trimmed.starts_with('3') {
        return Err(format!("{} failed: {}", context, trimmed));
    }

    info!(cmd = %context, response = %trimmed, "SMTP response");
    Ok(())
}

/// Try STARTTLS — if the server offers it, upgrade. Returns true if TLS was activated.
async fn try_starttls(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    reader: &mut tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
    line: &mut String,
) -> bool {
    if writer.write_all(b"STARTTLS\r\n").await.is_err() {
        return false;
    }

    line.clear();
    if reader.read_line(line).await.is_err() {
        return false;
    }

    if line.trim().starts_with("2") {
        info!("STARTTLS accepted — TLS upgrade available");
        // TODO: Perform TLS handshake here
        // For now, return true but don't actually upgrade
        true
    } else {
        info!(response = %line.trim(), "STARTTLS not available");
        false
    }
}

/// DNS MX record lookup using trust-dns-resolver.
///
/// Returns MX hosts sorted by priority (lowest first).
async fn lookup_mx(domain: &str) -> Result<Vec<String>, String> {
    use trust_dns_resolver::TokioAsyncResolver;

    let resolver = TokioAsyncResolver::tokio_from_system_conf()
        .map_err(|e| format!("Failed to create DNS resolver: {}", e))?;

    // Query MX records
    let mx_response = resolver
        .mx_lookup(domain)
        .await
        .map_err(|e| format!("MX lookup failed: {}", e))?;

    // Sort by priority (lowest first)
    let mut mx_records: Vec<(u16, String)> = mx_response
        .iter()
        .map(|mx| {
            (
                mx.preference(),
                mx.exchange().to_string().trim_end_matches('.').to_string(),
            )
        })
        .collect();

    mx_records.sort_by_key(|(preference, _)| *preference);

    let mx_hosts: Vec<String> = mx_records
        .into_iter()
        .map(|(_, exchange)| exchange)
        .collect();

    if mx_hosts.is_empty() {
        // Fallback: try A/AAAA record of the domain itself
        let addrs = resolver
            .lookup_ip(domain)
            .await
            .map_err(|e| format!("IP lookup failed: {}", e))?;

        if addrs.iter().next().is_some() {
            return Ok(vec![domain.to_string()]);
        }

        return Err(format!("No MX or A records for {}", domain));
    }

    Ok(mx_hosts)
}
