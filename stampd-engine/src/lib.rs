//! Stampd Engine — safety-critical core
//!
//! This library is used by Transit for cross-language function calls.
//! The binary (main.rs) is the standalone SMTP server.

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
