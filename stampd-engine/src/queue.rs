use crate::db::Database;
use crate::delivery;
use std::path::Path;
use std::sync::Arc;
use tracing::{info, warn};

const MAX_ATTEMPTS: i32 = 5;

pub async fn run(db: Arc<Database>, _maildir_path: String) -> anyhow::Result<()> {
    info!("Queue processor started");

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        // Process pending messages
        match db.get_pending_messages(10) {
            Ok(messages) => {
                for (id, _from_user_id, recipient, message_path, attempts) in &messages {
                    info!(id, recipient = %recipient, attempts, "Processing queued message");

                    // Check message file exists
                    if !Path::new(message_path).exists() {
                        warn!(id, "Message file missing, marking as failed");
                        if let Err(e) = db.mark_failed(*id, "Message file missing", MAX_ATTEMPTS) {
                            warn!(error = ?e, "Failed to mark message as failed");
                        }
                        continue;
                    }

                    // Read message content
                    let message = match tokio::fs::read(message_path).await {
                        Ok(m) => m,
                        Err(e) => {
                            warn!(id, error = ?e, "Failed to read message file");
                            let _ =
                                db.mark_failed(*id, &format!("Read error: {}", e), MAX_ATTEMPTS);
                            continue;
                        }
                    };

                    // Determine sender address from message (From: header)
                    let from = extract_from_header(&message)
                        .unwrap_or_else(|| "postmaster@localhost".to_string());

                    // Attempt delivery
                    match delivery::deliver(&from, recipient, &message).await {
                        delivery::DeliveryResult::Delivered => {
                            info!(id, recipient = %recipient, "Message delivered");
                            let _ = db.mark_delivered(*id);
                            let _ = db.log_delivery(*id, "delivered", recipient, None);
                        }
                        delivery::DeliveryResult::TemporaryFailure(err) => {
                            warn!(id, recipient = %recipient, error = %err, "Temporary delivery failure");
                            let _ = db.mark_failed(*id, &err, MAX_ATTEMPTS);
                            let _ = db.log_delivery(*id, "temp_failed", recipient, Some(&err));
                        }
                        delivery::DeliveryResult::PermanentFailure(err) => {
                            warn!(id, recipient = %recipient, error = %err, "Permanent delivery failure");
                            let _ = db.mark_failed(*id, &err, MAX_ATTEMPTS);
                            let _ = db.log_delivery(*id, "bounced", recipient, Some(&err));
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = ?e, "Failed to read pending messages");
            }
        }
    }
}

/// Extract the sender address from the From: header of a message.
fn extract_from_header(message: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(message);
    for line in text.lines() {
        if line.to_lowercase().starts_with("from:") {
            let addr = line[5..].trim();
            // Extract email from "Name <email>" format
            if let Some(start) = addr.find('<') {
                if let Some(end) = addr.find('>') {
                    return Some(addr[start + 1..end].to_string());
                }
            }
            return Some(addr.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_from_header_plain() {
        let msg = b"From: alice@foo.com\r\nSubject: test\r\n\r\nBody";
        assert_eq!(extract_from_header(msg).unwrap(), "alice@foo.com");
    }

    #[test]
    fn test_extract_from_header_with_name() {
        let msg = b"From: Alice <alice@foo.com>\r\nSubject: test\r\n\r\nBody";
        assert_eq!(extract_from_header(msg).unwrap(), "alice@foo.com");
    }

    #[test]
    fn test_extract_from_header_case_insensitive() {
        let msg = b"from: bob@bar.com\r\nSubject: test\r\n\r\nBody";
        assert_eq!(extract_from_header(msg).unwrap(), "bob@bar.com");
    }

    #[test]
    fn test_extract_from_header_no_from() {
        let msg = b"Subject: test\r\n\r\nBody";
        assert!(extract_from_header(msg).is_none());
    }

    #[test]
    fn test_extract_from_header_empty() {
        let msg = b"";
        assert!(extract_from_header(msg).is_none());
    }
}
