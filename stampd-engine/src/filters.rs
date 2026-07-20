//! Filter bridge for stampd-engine
//!
//! Calls user-defined filter hooks via Transit (Python runtime).
//! These hooks are invoked at MAIL FROM / RCPT TO / DATA stages.

use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
pub struct FilterResult {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
}

/// Call filter hook for MAIL FROM stage
pub async fn check_mail_from(sender: &str) -> anyhow::Result<FilterResult> {
    // TODO: Call Transit Python runtime
    // For now, return accept
    info!(sender = %sender, "Filter: MAIL FROM check (stub)");
    
    Ok(FilterResult {
        action: "accept".to_string(),
        reason: None,
        sender: Some(sender.to_string()),
        recipient: None,
    })
}

/// Call filter hook for RCPT TO stage
pub async fn check_rcpt_to(recipient: &str) -> anyhow::Result<FilterResult> {
    // TODO: Call Transit Python runtime
    info!(recipient = %recipient, "Filter: RCPT TO check (stub)");
    
    Ok(FilterResult {
        action: "accept".to_string(),
        reason: None,
        sender: None,
        recipient: Some(recipient.to_string()),
    })
}

/// Call filter hook for DATA stage
pub async fn check_data(_headers: &str, _body: &str) -> anyhow::Result<FilterResult> {
    // TODO: Call Transit Python runtime
    info!("Filter: DATA check (stub)");
    
    Ok(FilterResult {
        action: "accept".to_string(),
        reason: None,
        sender: None,
        recipient: None,
    })
}
