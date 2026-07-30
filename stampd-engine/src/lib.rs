//! Stampd Engine — safety-critical core
//!
//! This library is used by Transit for cross-language function calls.
//! The binary (main.rs) is the standalone SMTP server.

use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};

pub mod transit;
pub mod config;
pub mod db;
pub mod smtpd;
pub mod submissiond;
pub mod maildir;
pub mod queue;
pub mod filters;
pub mod delivery;
pub mod api;
pub mod tls;
pub mod spf;
pub mod dkim;
pub mod stats;

/// Global engine stats, accessible from napi exports.
/// Set once during init, then used by transit.rs.
pub static ENGINE_STATS: OnceLock<Arc<stats::EngineStats>> = OnceLock::new();
/// Global database handle, accessible from napi exports.
pub static ENGINE_DB: OnceLock<Arc<db::Database>> = OnceLock::new();
/// Flag indicating whether globals have been initialized.
pub static GLOBALS_INITIALIZED: AtomicBool = AtomicBool::new(false);
