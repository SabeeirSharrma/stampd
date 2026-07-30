//! Stampd Engine — safety-critical core
//!
//! This library is used by Transit for cross-language function calls.
//! The binary (main.rs) is the standalone SMTP server.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};

pub mod api;
pub mod config;
pub mod db;
pub mod delivery;
pub mod dkim;
pub mod filters;
pub mod maildir;
pub mod queue;
pub mod smtpd;
pub mod spf;
pub mod stats;
pub mod submissiond;
pub mod tls;
pub mod transit;

/// Global engine stats, accessible from napi exports.
/// Set once during init, then used by transit.rs.
pub static ENGINE_STATS: OnceLock<Arc<stats::EngineStats>> = OnceLock::new();
/// Global database handle, accessible from napi exports.
pub static ENGINE_DB: OnceLock<Arc<db::Database>> = OnceLock::new();
/// Flag indicating whether globals have been initialized.
pub static GLOBALS_INITIALIZED: AtomicBool = AtomicBool::new(false);
