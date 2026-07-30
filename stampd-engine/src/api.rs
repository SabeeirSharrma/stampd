//! Internal HTTP API for the engine.
//!
//! Exposes queue status, SMTP stats, and config reload for the gateway.
//! Binds to 127.0.0.1 only — not publicly exposed.

use axum::{routing::get, Json, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;
use serde_json::{json, Value};

use crate::db::Database;
use crate::stats::EngineStats;

pub async fn run(port: u16, db: Arc<Database>, stats: Arc<EngineStats>) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(health))
        .route("/api/queue/stats", get(queue_stats))
        .route("/api/smtp/stats", get(smtp_stats))
        .route("/api/config", get(get_config))
        .with_state((db, stats));

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!(port, "Engine internal API listening on 127.0.0.1");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "stampd-engine" }))
}

async fn queue_stats(
    axum::extract::State((db, _stats)): axum::extract::State<(Arc<Database>, Arc<EngineStats>)>,
) -> Json<Value> {
    match db.queue_stats() {
        Ok((pending, delivered, dead)) => Json(json!({
            "pending": pending,
            "delivered": delivered,
            "dead": dead,
        })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

async fn smtp_stats(
    axum::extract::State((_db, stats)): axum::extract::State<(Arc<Database>, Arc<EngineStats>)>,
) -> Json<Value> {
    let (total, active, received, sent, failed) = stats.snapshot();
    Json(json!({
        "connections_total": total,
        "connections_active": active,
        "messages_received": received,
        "messages_sent": sent,
        "messages_sent_failed": failed,
    }))
}

async fn get_config(
    axum::extract::State((db, _stats)): axum::extract::State<(Arc<Database>, Arc<EngineStats>)>,
) -> Json<Value> {
    match db.get_server_config() {
        Ok((domain, signup_enabled, dkim_selector)) => Json(json!({
            "domain": domain,
            "signup_enabled": signup_enabled,
            "dkim_selector": dkim_selector,
        })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}
