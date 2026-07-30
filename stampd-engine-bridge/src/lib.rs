//! Stampd Engine Bridge — Transit napi interface.
//!
//! Exposes core engine functions to Node.js via Transit.
//! Gateway loads this as a native addon for zero-overhead function calls.

use napi_derive::napi;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use std::path::Path;

/// Database wrapper for napi.
#[napi]
pub struct StampdDb {
    conn: Arc<Mutex<Connection>>,
}

#[napi]
impl StampdDb {
    /// Open a Stampd database.
    #[napi(factory)]
    pub fn open(db_path: String) -> napi::Result<Self> {
        let conn = Connection::open(&db_path)
            .map_err(|e| napi::Error::from_reason(format!("Failed to open database: {}", e)))?;

        // Enable WAL mode for concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| napi::Error::from_reason(format!("Failed to set WAL mode: {}", e)))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Get queue statistics (pending, delivered, dead).
    #[napi]
    pub fn queue_stats(&self) -> napi::Result<QueueStats> {
        let conn = self.conn.lock()
            .map_err(|e| napi::Error::from_reason(format!("Lock error: {}", e)))?;

        let pending: i64 = conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE status = 'pending'",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        let delivered: i64 = conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE status = 'delivered'",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        let dead: i64 = conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE status = 'dead'",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        Ok(QueueStats {
            pending: pending as u32,
            delivered: delivered as u32,
            dead: dead as u32,
        })
    }

    /// Get server configuration.
    #[napi]
    pub fn get_config(&self) -> napi::Result<ServerConfig> {
        let conn = self.conn.lock()
            .map_err(|e| napi::Error::from_reason(format!("Lock error: {}", e)))?;

        let result: Result<(String, bool, String), _> = conn.query_row(
            "SELECT value FROM server_config WHERE key = 'domain' UNION ALL \
             SELECT value FROM server_config WHERE key = 'signup_enabled' UNION ALL \
             SELECT value FROM server_config WHERE key = 'dkim_selector'",
            [],
            |row| {
                let domain: String = row.get(0)?;
                let signup: String = row.get(0)?;
                let dkim: String = row.get(0)?;
                Ok((domain, signup == "true", dkim))
            },
        );

        // Simpler approach: query each key separately
        let domain: String = conn.query_row(
            "SELECT value FROM server_config WHERE key = 'domain'",
            [],
            |row| row.get(0),
        ).unwrap_or_else(|_| "localhost".to_string());

        let signup_enabled: bool = conn.query_row(
            "SELECT value FROM server_config WHERE key = 'signup_enabled'",
            [],
            |row| row.get::<_, String>(0),
        ).map(|v| v == "true").unwrap_or(true);

        let dkim_selector: String = conn.query_row(
            "SELECT value FROM server_config WHERE key = 'dkim_selector'",
            [],
            |row| row.get(0),
        ).unwrap_or_else(|_| "default".to_string());

        Ok(ServerConfig {
            domain,
            signup_enabled,
            dkim_selector,
        })
    }

    /// Check if a domain is allowed (configured domain or verified custom domain).
    #[napi]
    pub fn is_domain_allowed(&self, domain: String) -> napi::Result<bool> {
        let conn = self.conn.lock()
            .map_err(|e| napi::Error::from_reason(format!("Lock error: {}", e)))?;

        // Check configured domain
        let config_domain: String = conn.query_row(
            "SELECT value FROM server_config WHERE key = 'domain'",
            [],
            |row| row.get(0),
        ).unwrap_or_else(|_| "localhost".to_string());

        if domain.to_lowercase() == config_domain.to_lowercase() {
            return Ok(true);
        }

        // Check custom domains
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM custom_domains WHERE domain = ? AND verified = 1",
            [domain.to_lowercase()],
            |row| row.get(0),
        ).unwrap_or(0);

        Ok(count > 0)
    }

    /// Get the owner of a custom domain.
    #[napi]
    pub fn get_domain_owner(&self, domain: String) -> napi::Result<Option<String>> {
        let conn = self.conn.lock()
            .map_err(|e| napi::Error::from_reason(format!("Lock error: {}", e)))?;

        let result: Result<String, _> = conn.query_row(
            "SELECT u.email FROM users u \
             JOIN custom_domains cd ON cd.user_id = u.id \
             WHERE cd.domain = ? AND cd.verified = 1",
            [domain.to_lowercase()],
            |row| row.get(0),
        );

        match result {
            Ok(email) => Ok(Some(email)),
            Err(_) => Ok(None),
        }
    }

    /// List all custom domains for a user.
    #[napi]
    pub fn list_custom_domains(&self, user_id: i64) -> napi::Result<Vec<CustomDomain>> {
        let conn = self.conn.lock()
            .map_err(|e| napi::Error::from_reason(format!("Lock error: {}", e)))?;

        let mut stmt = conn.prepare(
            "SELECT id, domain, user_id, verified, created_at \
             FROM custom_domains WHERE user_id = ? ORDER BY id"
        ).map_err(|e| napi::Error::from_reason(format!("Prepare error: {}", e)))?;

        let domains = stmt.query_map([user_id], |row| {
            Ok(CustomDomain {
                id: row.get(0)?,
                domain: row.get(1)?,
                user_id: row.get(2)?,
                verified: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|e| napi::Error::from_reason(format!("Query error: {}", e)))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(domains)
    }

    /// Get user by ID.
    #[napi]
    pub fn get_user(&self, user_id: i64) -> napi::Result<Option<User>> {
        let conn = self.conn.lock()
            .map_err(|e| napi::Error::from_reason(format!("Lock error: {}", e)))?;

        let result: Result<User, _> = conn.query_row(
            "SELECT id, email, is_admin, disabled_at FROM users WHERE id = ?",
            [user_id],
            |row| {
                Ok(User {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    is_admin: row.get(2)?,
                    disabled_at: row.get(3)?,
                })
            },
        );

        match result {
            Ok(user) => Ok(Some(user)),
            Err(_) => Ok(None),
        }
    }
}

/// Queue statistics.
#[napi(object)]
pub struct QueueStats {
    pub pending: u32,
    pub delivered: u32,
    pub dead: u32,
}

/// Server configuration.
#[napi(object)]
pub struct ServerConfig {
    pub domain: String,
    pub signup_enabled: bool,
    pub dkim_selector: String,
}

/// Custom domain record.
#[napi(object)]
pub struct CustomDomain {
    pub id: i64,
    pub domain: String,
    pub user_id: i64,
    pub verified: bool,
    pub created_at: i64,
}

/// User record.
#[napi(object)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub is_admin: bool,
    pub disabled_at: Option<i64>,
}
