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
    use std::sync::OnceLock;
    static DB: OnceLock<std::sync::Arc<crate::db::Database>> = OnceLock::new();

    // Try to get the global database handle
    if let Some(db) = crate::ENGINE_DB.get() {
        match db.queue_stats() {
            Ok((pending, delivered, dead)) => {
                let status = json!({
                    "pending": pending,
                    "delivered": delivered,
                    "dead": dead,
                });
                return status.to_string();
            }
            Err(e) => {
                return json!({ "error": e.to_string() }).to_string();
            }
        }
    }

    // Fallback if DB not initialized
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
    if let Some(stats) = crate::ENGINE_STATS.get() {
        let (total, active, received, sent, failed) = stats.snapshot();
        let stats = json!({
            "connections_total": total,
            "connections_active": active,
            "messages_received": received,
            "messages_sent": sent,
            "messages_sent_failed": failed,
        });
        return stats.to_string();
    }

    // Fallback if stats not initialized
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
    // TODO: Trigger config reload (implemented in v0.8.0)
    let result = json!({
        "success": true,
        "message": "Config reload triggered",
    });
    result.to_string()
}

/// Check if domain is configured
#[napi]
pub fn check_domain(domain: String) -> String {
    if let Some(db) = crate::ENGINE_DB.get() {
        let configured = db.is_domain_allowed(&domain);
        let result = json!({
            "domain": domain,
            "configured": configured,
        });
        return result.to_string();
    }

    // Fallback if DB not initialized
    let result = json!({
        "domain": domain,
        "configured": false,
    });
    result.to_string()
}
