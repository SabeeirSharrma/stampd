use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, warn, error};
use std::sync::Arc;
use crate::db::Database;

pub async fn run(port: u16, _dkim_selector: String, _db: Arc<Database>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!(port, "Submission server listening");

    loop {
        let (stream, addr) = listener.accept().await?;
        info!(addr = %addr, "New submission connection");

        tokio::spawn(async move {
            if let Err(e) = handle_submission(stream, addr).await {
                error!(addr = %addr, error = ?e, "Submission error");
            }
        });
    }
}

async fn handle_submission(
    stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Send greeting
    writer.write_all(b"220 Stampd Submission ready\r\n").await?;

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break;
        }

        let cmd = line.trim();
        info!(addr = %addr, cmd = %cmd, "Received submission command");

        // TODO: Parse SMTP commands with AUTH requirement
        match cmd.split_whitespace().next().unwrap_or("") {
            "HELO" | "EHLO" => {
                writer.write_all(b"250-Stampd\r\n250-8BITMIME\r\n250-AUTH PLAIN LOGIN\r\n250-STARTTLS\r\n250 OK\r\n").await?;
            }
            "AUTH" => {
                // TODO: Implement AUTH PLAIN/LOGIN
                writer.write_all(b"235 Authentication successful\r\n").await?;
            }
            "STARTTLS" => {
                // TODO: Implement STARTTLS
                writer.write_all(b"454 TLS not available\r\n").await?;
            }
            "MAIL" => {
                // TODO: Parse MAIL FROM
                writer.write_all(b"250 OK\r\n").await?;
            }
            "RCPT" => {
                // TODO: Parse RCPT TO
                writer.write_all(b"250 OK\r\n").await?;
            }
            "DATA" => {
                // TODO: Read message body and sign with DKIM
                writer.write_all(b"354 Start mail input\r\n").await?;
                // Read until \r\n.\r\n
                loop {
                    line.clear();
                    reader.read_line(&mut line).await?;
                    if line.trim() == "." {
                        break;
                    }
                }
                writer.write_all(b"250 OK\r\n").await?;
            }
            "QUIT" => {
                writer.write_all(b"221 Bye\r\n").await?;
                break;
            }
            "RSET" => {
                writer.write_all(b"250 OK\r\n").await?;
            }
            _ => {
                warn!(addr = %addr, cmd = %cmd, "Unknown command");
                writer.write_all(b"500 Command not recognized\r\n").await?;
            }
        }
    }

    info!(addr = %addr, "Submission connection closed");
    Ok(())
}
