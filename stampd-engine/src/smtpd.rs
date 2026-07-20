use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, warn, error};

pub async fn run(port: u16, maildir_path: String, domain: String) -> anyhow::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!(port, "SMTP server listening");

    loop {
        let (stream, addr) = listener.accept().await?;
        info!(addr = %addr, "New connection");

        let maildir_path = maildir_path.clone();
        let domain = domain.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, addr, maildir_path, domain).await {
                error!(addr = %addr, error = ?e, "Connection error");
            }
        });
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
    _maildir_path: String,
    _domain: String,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Send greeting
    writer.write_all(b"220 Stampd ESMTP ready\r\n").await?;

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // Connection closed
        }

        let cmd = line.trim();
        info!(addr = %addr, cmd = %cmd, "Received command");

        // TODO: Parse SMTP commands (HELO, EHLO, MAIL FROM, RCPT TO, DATA, QUIT)
        match cmd.split_whitespace().next().unwrap_or("") {
            "HELO" | "EHLO" => {
                writer.write_all(b"250-Stampd\r\n250-8BITMIME\r\n250-STARTTLS\r\n250 OK\r\n").await?;
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
                // TODO: Parse RCPT TO and validate domain
                writer.write_all(b"250 OK\r\n").await?;
            }
            "DATA" => {
                // TODO: Read message body
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

    info!(addr = %addr, "Connection closed");
    Ok(())
}
