use std::path::{Path, PathBuf};
use tracing::info;
use anyhow::Result;

/// Initialize Maildir structure for all users
pub async fn init(maildir_path: &str) -> Result<()> {
    let base = Path::new(maildir_path);
    tokio::fs::create_dir_all(base).await?;
    info!(path = %maildir_path, "Maildir initialized");
    Ok(())
}

/// Create Maildir for a new user
pub async fn create_user_mailbox(maildir_path: &str, domain: &str, user: &str) -> Result<PathBuf> {
    let user_dir = Path::new(maildir_path).join(domain).join(user);
    for subdir in &["cur", "new", "tmp", "sent", "drafts", "archive", "spam"] {
        tokio::fs::create_dir_all(user_dir.join(subdir)).await?;
    }
    info!(domain, user, "Created user mailbox");
    Ok(user_dir)
}

/// Generate unique Maildir filename
pub fn generate_maildir_filename() -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let pid = std::process::id();
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "localhost".to_string());
    format!("{}.{}.{}", timestamp, pid, hostname)
}

/// Save message to Maildir
pub async fn save_message(
    maildir_path: &str,
    domain: &str,
    user: &str,
    message_id: &str,
    content: &[u8],
) -> Result<PathBuf> {
    let filename = format!("{}.{}", message_id, generate_maildir_filename());
    let filepath = Path::new(maildir_path)
        .join(domain)
        .join(user)
        .join("new")
        .join(&filename);
    tokio::fs::write(&filepath, content).await?;
    info!(domain, user, filename, "Saved message");
    Ok(filepath)
}
