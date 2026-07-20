//! Transit exports for stampd-engine
//!
//! These functions are exported as napi functions (#[napi]) so Transit's
//! RustDevBridge can load them as a native addon (.node/.so).
//! The gateway calls these via transit.rust().

use napi_derive::napi;
use serde_json::json;

/// Get delivery queue status
#[napi]
pub fn get_queue_status() -> String {
    // TODO: Query actual delivery queue
    let status = json!({
        "pending": 0,
        "delivered": 0,
        "dead": 0,
    });
    status.to_string()
}

/// Get SMTP connection stats
#[napi]
pub fn get_smtp_stats() -> String {
    // TODO: Query actual SMTP stats
    let stats = json!({
        "connections_total": 0,
        "connections_active": 0,
        "messages_received": 0,
        "messages_sent": 0,
    });
    stats.to_string()
}

/// Trigger config reload
#[napi]
pub fn reload_config() -> String {
    // TODO: Trigger config reload
    let result = json!({
        "success": true,
        "message": "Config reload triggered",
    });
    result.to_string()
}

/// Check if domain is configured
#[napi]
pub fn check_domain(domain: String) -> String {
    // TODO: Check if domain is configured
    let result = json!({
        "domain": domain,
        "configured": true,
    });
    result.to_string()
}
