use tracing::{info, warn};
use std::sync::Arc;
use std::path::Path;
use crate::db::Database;

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
                        if let Err(e) = db.mark_failed(*id, "Message file missing", 5) {
                            warn!(error = ?e, "Failed to mark message as failed");
                        }
                        continue;
                    }

                    // TODO: Phase 0.2.6 — actual MX lookup and SMTP delivery
                    // For now, mark as delivered (stub)
                    info!(id, recipient = %recipient, "Delivery stub — marking as delivered");
                    if let Err(e) = db.mark_delivered(*id) {
                        warn!(error = ?e, "Failed to mark message as delivered");
                    }
                    if let Err(e) = db.log_delivery(*id, "delivered", recipient, None) {
                        warn!(error = ?e, "Failed to log delivery");
                    }
                }
            }
            Err(e) => {
                warn!(error = ?e, "Failed to read pending messages");
            }
        }
    }
}
