//! Internal HTTP API for the engine.
//!
//! Exposes queue status, SMTP stats, and config reload for the gateway.
//! Binds to 127.0.0.1 only — not publicly exposed.

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::config::Config;
use crate::db::Database;
use crate::stats::EngineStats;

/// Shared state for the API.
pub struct ApiState {
    pub db: Arc<Database>,
    pub stats: Arc<EngineStats>,
    pub config: Arc<RwLock<Config>>,
    pub config_path: PathBuf,
}

pub async fn run(
    port: u16,
    db: Arc<Database>,
    stats: Arc<EngineStats>,
    config: Arc<RwLock<Config>>,
    config_path: PathBuf,
) -> anyhow::Result<()> {
    let state = Arc::new(ApiState {
        db,
        stats,
        config,
        config_path,
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/queue/stats", get(queue_stats))
        .route("/api/smtp/stats", get(smtp_stats))
        .route("/api/config", get(get_config))
        .route("/api/config/reload", post(reload_config))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!(port, "Engine internal API listening on 127.0.0.1");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "stampd-engine" }))
}

async fn queue_stats(State(state): State<Arc<ApiState>>) -> Json<Value> {
    match state.db.queue_stats() {
        Ok((pending, delivered, dead)) => Json(json!({
            "pending": pending,
            "delivered": delivered,
            "dead": dead,
        })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

async fn smtp_stats(State(state): State<Arc<ApiState>>) -> Json<Value> {
    let (total, active, received, sent, failed) = state.stats.snapshot();
    Json(json!({
        "connections_total": total,
        "connections_active": active,
        "messages_received": received,
        "messages_sent": sent,
        "messages_sent_failed": failed,
    }))
}

async fn get_config(State(state): State<Arc<ApiState>>) -> Json<Value> {
    let config = state.config.read().await;
    Json(json!({
        "domain": config.engine.domain,
        "signup_enabled": true,
        "dkim_selector": config.engine.dkim_selector,
    }))
}

async fn reload_config(State(state): State<Arc<ApiState>>) -> Json<Value> {
    match Config::load(&state.config_path) {
        Ok(new_config) => {
            let mut config = state.config.write().await;
            *config = new_config;
            info!("Configuration reloaded via API");
            Json(json!({ "ok": true, "message": "Configuration reloaded" }))
        }
        Err(e) => Json(json!({ "ok": false, "error": format!("Failed to reload config: {}", e) })),
    }
}
