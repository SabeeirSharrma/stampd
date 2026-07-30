//! SQLite database layer for Stampd.
//!
//! All metadata, auth, and routing lives here.
//! Mail content lives in Maildir — never in SQLite.

use rusqlite::{Connection, Result as SqlResult};
use std::path::Path;
use std::sync::Mutex;
use tracing::info;

/// Shared database handle, safe to pass across async tasks.
pub struct Database {
    conn: Mutex<Connection>,
}

/// Schema version — bump when migrations change.
#[allow(dead_code)]
const SCHEMA_VERSION: i32 = 1;

impl Database {
    /// Open (or create) the database and run migrations.
    pub fn open(path: &Path) -> SqlResult<Self> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        // Foreign keys must be enabled per-connection
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.run_migrations()?;
        Ok(db)
    }

    /// Run all pending migrations, idempotent.
    fn run_migrations(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();

        // Create migration tracking table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                version INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );",
        )?;

        let current: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _migrations",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if current < 1 {
            conn.execute_batch(SCHEMA_V1)?;
            conn.execute(
                "INSERT INTO _migrations (version, applied_at) VALUES (1, ?1)",
                [now()],
            )?;
            info!("Applied migration v1");
        }

        Ok(())
    }

    // ── Server Config ─────────────────────────────────────────────

    /// Get the server config (singleton row). Returns (domain, signup_enabled, dkim_selector).
    pub fn get_server_config(&self) -> SqlResult<(String, bool, String)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT domain, signup_enabled, dkim_selector FROM server_config WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get::<_, i32>(1)? == 1, row.get(2)?)),
        )
    }

    /// Update server config fields.
    pub fn update_server_config(
        &self,
        domain: Option<&str>,
        signup_enabled: Option<bool>,
        dkim_selector: Option<&str>,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        if let Some(d) = domain {
            conn.execute("UPDATE server_config SET domain = ?1 WHERE id = 1", [d])?;
        }
        if let Some(s) = signup_enabled {
            conn.execute(
                "UPDATE server_config SET signup_enabled = ?1 WHERE id = 1",
                [s as i32],
            )?;
        }
        if let Some(sel) = dkim_selector {
            conn.execute(
                "UPDATE server_config SET dkim_selector = ?1 WHERE id = 1",
                [sel],
            )?;
        }
        Ok(())
    }

    // ── Users ─────────────────────────────────────────────────────

    /// Create a new user. Returns the user id.
    pub fn create_user(&self, email: &str, password_hash: &str, is_admin: bool) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (email, password_hash, is_admin, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![email, password_hash, is_admin as i32, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get a user by email. Returns (id, password_hash, is_admin, disabled).
    pub fn get_user_by_email(&self, email: &str) -> SqlResult<Option<(i64, String, bool, bool)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, password_hash, is_admin, disabled_at IS NOT NULL FROM users WHERE email = ?1",
        )?;
        let mut rows = stmt.query([email])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, i32>(2)? == 1,
                row.get::<_, i32>(3)? == 1,
            )))
        } else {
            Ok(None)
        }
    }

    /// Get a user by id. Returns (email, is_admin, disabled).
    pub fn get_user_by_id(&self, id: i64) -> SqlResult<Option<(String, bool, bool)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT email, is_admin, disabled_at IS NOT NULL FROM users WHERE id = ?1")?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get(0)?,
                row.get::<_, i32>(1)? == 1,
                row.get::<_, i32>(2)? == 1,
            )))
        } else {
            Ok(None)
        }
    }

    /// List all users. Returns (id, email, is_admin, disabled).
    pub fn list_users(&self) -> SqlResult<Vec<(i64, String, bool, bool)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, email, is_admin, disabled_at IS NOT NULL FROM users ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, i32>(2)? == 1,
                row.get::<_, i32>(3)? == 1,
            ))
        })?;
        rows.collect()
    }

    /// Disable a user (set disabled_at).
    pub fn disable_user(&self, id: i64) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE users SET disabled_at = ?1 WHERE id = ?2 AND disabled_at IS NULL",
            [now(), id],
        )?;
        Ok(affected > 0)
    }

    /// Delete a user and cascade-delete their tokens and sessions.
    pub fn delete_user(&self, id: i64) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        // Cascade in correct order (FK constraints handle this, but be explicit)
        conn.execute("DELETE FROM sessions WHERE user_id = ?1", [id])?;
        conn.execute("DELETE FROM auth_tokens WHERE user_id = ?1", [id])?;
        let affected = conn.execute("DELETE FROM users WHERE id = ?1", [id])?;
        Ok(affected > 0)
    }

    // ── Auth Tokens ───────────────────────────────────────────────

    /// Create a token. Returns (token_id, raw_token).
    pub fn create_token(
        &self,
        user_id: i64,
        token_hash: &str,
        label: &str,
        scope: &str,
    ) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO auth_tokens (user_id, token_hash, label, scope, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![user_id, token_hash, label, scope, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// List tokens for a user. Returns (id, label, scope, created_at, last_used_at, revoked).
    #[allow(clippy::type_complexity)]
    pub fn list_user_tokens(
        &self,
        user_id: i64,
    ) -> SqlResult<Vec<(i64, String, String, i64, Option<i64>, bool)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, label, scope, created_at, last_used_at, revoked_at IS NOT NULL
             FROM auth_tokens WHERE user_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([user_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get::<_, i32>(5)? == 1,
            ))
        })?;
        rows.collect()
    }

    /// List all tokens (admin view). Returns (id, user_id, label, scope, created_at, revoked).
    #[allow(clippy::type_complexity)]
    pub fn list_all_tokens(&self) -> SqlResult<Vec<(i64, i64, String, String, i64, bool)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, label, scope, created_at, revoked_at IS NOT NULL
             FROM auth_tokens ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get::<_, i32>(5)? == 1,
            ))
        })?;
        rows.collect()
    }

    /// Validate a token by hash. Returns (token_id, user_id) if valid and not revoked.
    pub fn validate_token(&self, token_hash: &str) -> SqlResult<Option<(i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id FROM auth_tokens
             WHERE token_hash = ?1 AND revoked_at IS NULL",
        )?;
        let mut rows = stmt.query([token_hash])?;
        if let Some(row) = rows.next()? {
            let token_id: i64 = row.get(0)?;
            let user_id: i64 = row.get(1)?;
            // Update last_used_at
            conn.execute(
                "UPDATE auth_tokens SET last_used_at = ?1 WHERE id = ?2",
                rusqlite::params![now(), token_id],
            )?;
            Ok(Some((token_id, user_id)))
        } else {
            Ok(None)
        }
    }

    /// Revoke a token. Returns true if it existed and was not already revoked.
    pub fn revoke_token(&self, id: i64) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE auth_tokens SET revoked_at = ?1 WHERE id = ?2 AND revoked_at IS NULL",
            [now(), id],
        )?;
        Ok(affected > 0)
    }

    // ── Sessions ──────────────────────────────────────────────────

    /// Create a session. Returns the session id.
    pub fn create_session(&self, user_id: i64, expires_at: i64) -> SqlResult<String> {
        let conn = self.conn.lock().unwrap();
        let id = uuid_v4();
        conn.execute(
            "INSERT INTO sessions (id, user_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, user_id, now(), expires_at],
        )?;
        Ok(id)
    }

    /// Validate a session. Returns user_id if valid and not expired.
    pub fn validate_session(&self, session_id: &str) -> SqlResult<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let now_ts = now();
        let mut stmt =
            conn.prepare("SELECT user_id FROM sessions WHERE id = ?1 AND expires_at > ?2")?;
        let mut rows = stmt.query(rusqlite::params![session_id, now_ts])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Delete a session.
    pub fn delete_session(&self, session_id: &str) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
        Ok(affected > 0)
    }

    // ── Delivery Queue ────────────────────────────────────────────

    /// Enqueue a message for delivery. Returns the queue entry id.
    pub fn enqueue(
        &self,
        from_user_id: i64,
        recipient: &str,
        message_path: &str,
    ) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO delivery_queue (from_user_id, recipient, message_path, attempts, next_attempt_at, status)
             VALUES (?1, ?2, ?3, 0, ?4, 'pending')",
            rusqlite::params![from_user_id, recipient, message_path, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get pending messages ready for delivery attempt.
    #[allow(clippy::type_complexity)]
    pub fn get_pending_messages(
        &self,
        limit: i64,
    ) -> SqlResult<Vec<(i64, i64, String, String, i32)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, from_user_id, recipient, message_path, attempts
             FROM delivery_queue
             WHERE status = 'pending' AND next_attempt_at <= ?1
             ORDER BY next_attempt_at
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![now(), limit], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?;
        rows.collect()
    }

    /// Mark a delivery as successful.
    pub fn mark_delivered(&self, id: i64) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE delivery_queue SET status = 'delivered' WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    /// Mark a delivery as failed and schedule retry (exponential backoff).
    pub fn mark_failed(&self, id: i64, error: &str, max_attempts: i32) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        let row: (i32, i64) = conn.query_row(
            "SELECT attempts, next_attempt_at FROM delivery_queue WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let new_attempts = row.0 + 1;
        if new_attempts >= max_attempts {
            // Dead-letter
            conn.execute(
                "UPDATE delivery_queue SET status = 'dead', last_error = ?1, attempts = ?2 WHERE id = ?3",
                rusqlite::params![error, new_attempts, id],
            )?;
        } else {
            // Exponential backoff: 30s, 60s, 120s, 240s, ... capped at 1 hour
            let backoff_secs = std::cmp::min(30 * 2i64.pow((new_attempts - 1) as u32), 3600);
            let next_attempt = now() + backoff_secs;
            conn.execute(
                "UPDATE delivery_queue SET attempts = ?1, next_attempt_at = ?2, last_error = ?3 WHERE id = ?4",
                rusqlite::params![new_attempts, next_attempt, error, id],
            )?;
        }
        Ok(())
    }

    /// Get queue stats: (pending, delivered, dead).
    pub fn queue_stats(&self) -> SqlResult<(i64, i64, i64)> {
        let conn = self.conn.lock().unwrap();
        let pending: i64 = conn.query_row(
            "SELECT COUNT(*) FROM delivery_queue WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )?;
        let delivered: i64 = conn.query_row(
            "SELECT COUNT(*) FROM delivery_queue WHERE status = 'delivered'",
            [],
            |row| row.get(0),
        )?;
        let dead: i64 = conn.query_row(
            "SELECT COUNT(*) FROM delivery_queue WHERE status = 'dead'",
            [],
            |row| row.get(0),
        )?;
        Ok((pending, delivered, dead))
    }

    /// List dead-lettered messages for admin review.
    #[allow(clippy::type_complexity)]
    pub fn list_dead_letters(
        &self,
    ) -> SqlResult<Vec<(i64, i64, String, String, i32, Option<String>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, from_user_id, recipient, message_path, attempts, last_error
             FROM delivery_queue WHERE status = 'dead' ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?;
        rows.collect()
    }

    /// Retry a dead-lettered message (reset to pending).
    pub fn retry_message(&self, id: i64) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE delivery_queue SET status = 'pending', next_attempt_at = ?1 WHERE id = ?2 AND status = 'dead'",
            [now(), id],
        )?;
        Ok(affected > 0)
    }

    /// Purge a message from the queue.
    pub fn purge_message(&self, id: i64) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM delivery_queue WHERE id = ?1", [id])?;
        Ok(affected > 0)
    }

    // ── Delivery Logs ─────────────────────────────────────────────

    /// Log a delivery event.
    pub fn log_delivery(
        &self,
        queue_id: i64,
        status: &str,
        recipient: &str,
        error: Option<&str>,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO delivery_logs (queue_id, status, recipient, error, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![queue_id, status, recipient, error, now()],
        )?;
        Ok(())
    }

    /// Get recent delivery logs.
    #[allow(clippy::type_complexity)]
    pub fn get_delivery_logs(
        &self,
        limit: i64,
    ) -> SqlResult<Vec<(i64, i64, String, String, Option<String>, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, queue_id, status, recipient, error, created_at
             FROM delivery_logs ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?;
        rows.collect()
    }

    // ── Filters ───────────────────────────────────────────────────

    /// List all filters.
    #[allow(clippy::type_complexity)]
    pub fn list_filters(&self) -> SqlResult<Vec<(i64, String, String, String, bool, i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, hooks, enabled, created_at, updated_at FROM filters ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get::<_, i64>(4)? != 0,
                row.get(5)?,
                row.get(6)?,
            ))
        })?;
        rows.collect()
    }

    /// Get a filter by id.
    pub fn get_filter(&self, id: i64) -> SqlResult<(i64, String, String, String, bool, i64, i64)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, path, hooks, enabled, created_at, updated_at FROM filters WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get::<_, i64>(4)? != 0,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
    }

    /// Insert a new filter.
    pub fn create_filter(&self, name: &str, path: &str, hooks: &str) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        let ts = now();
        conn.execute(
            "INSERT INTO filters (name, path, hooks, enabled, created_at, updated_at) VALUES (?1, ?2, ?3, 1, ?4, ?4)",
            rusqlite::params![name, path, hooks, ts],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update filter enabled state.
    pub fn set_filter_enabled(&self, id: i64, enabled: bool) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE filters SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id, enabled as i64, now()],
        )?;
        Ok(rows > 0)
    }

    /// Delete a filter.
    pub fn delete_filter(&self, id: i64) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute("DELETE FROM filters WHERE id = ?1", [id])?;
        Ok(rows > 0)
    }

    // ── Custom Domains ───────────────────────────────────────────

    /// Check if a domain is allowed (configured domain or a verified custom domain).
    pub fn is_domain_allowed(&self, domain: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        // Check configured domain
        let configured: String = conn
            .query_row("SELECT domain FROM server_config WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap_or_default();
        if domain.eq_ignore_ascii_case(&configured) {
            return true;
        }
        // Check custom domains
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM custom_domains WHERE domain = ?1 AND verified = 1",
                [domain.to_lowercase()],
                |row| row.get(0),
            )
            .unwrap_or(0);
        count > 0
    }

    /// Get the user_id that owns a custom domain (for routing incoming mail).
    pub fn get_domain_owner(&self, domain: &str) -> Option<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT user_id FROM custom_domains WHERE domain = ?1 AND verified = 1",
            [domain.to_lowercase()],
            |row| row.get(0),
        )
        .ok()
    }

    /// Get the local part (username) for a domain+email combo.
    /// For custom domains, the user's email local part is used.
    pub fn get_mailbox_user_for_domain(
        &self,
        domain: &str,
        recipient_email: &str,
    ) -> Option<String> {
        let local_part = recipient_email.split('@').next()?.to_string();
        let config_domain: String = {
            let conn = self.conn.lock().unwrap();
            conn.query_row("SELECT domain FROM server_config WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap_or_default()
        };
        if domain.eq_ignore_ascii_case(&config_domain) {
            return Some(local_part);
        }
        // For custom domains, find the user who owns this domain
        let owner_id = self.get_domain_owner(domain)?;
        let conn = self.conn.lock().unwrap();
        let email: String = conn
            .query_row("SELECT email FROM users WHERE id = ?1", [owner_id], |row| {
                row.get(0)
            })
            .ok()?;
        Some(email.split('@').next()?.to_string())
    }

    /// List all custom domains for a user.
    pub fn list_custom_domains(&self, user_id: i64) -> SqlResult<Vec<(i64, String, bool, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, domain, verified, created_at FROM custom_domains WHERE user_id = ?1 ORDER BY id"
        )?;
        let rows = stmt.query_map([user_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, i32>(2)? == 1,
                row.get(3)?,
            ))
        })?;
        rows.collect()
    }

    /// Add a custom domain for a user.
    pub fn add_custom_domain(&self, user_id: i64, domain: &str) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        let _result = conn.execute(
            "INSERT INTO custom_domains (domain, user_id, verified, created_at) VALUES (?1, ?2, 0, ?3)",
            rusqlite::params![domain.to_lowercase(), user_id, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Verify a custom domain (admin action or DNS check).
    pub fn verify_custom_domain(&self, id: i64) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute("UPDATE custom_domains SET verified = 1 WHERE id = ?1", [id])?;
        Ok(rows > 0)
    }

    /// Delete a custom domain.
    pub fn delete_custom_domain(&self, id: i64) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute("DELETE FROM custom_domains WHERE id = ?1", [id])?;
        Ok(rows > 0)
    }

    /// List all verified custom domains (for the engine to accept mail for).
    pub fn list_verified_domains(&self) -> Vec<String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT domain FROM custom_domains WHERE verified = 1")
            .unwrap();
        let rows = stmt.query_map([], |row| row.get(0)).unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }
}

// ── Schema SQL ───────────────────────────────────────────────────

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    is_admin BOOLEAN NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    disabled_at INTEGER
);

CREATE TABLE IF NOT EXISTS auth_tokens (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    token_hash TEXT NOT NULL,
    label TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT 'send',
    created_at INTEGER NOT NULL,
    last_used_at INTEGER,
    revoked_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_auth_tokens_user_id ON auth_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_auth_tokens_hash ON auth_tokens(token_hash);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions(user_id);

CREATE TABLE IF NOT EXISTS server_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    domain TEXT NOT NULL,
    signup_enabled BOOLEAN NOT NULL DEFAULT 1,
    dkim_selector TEXT NOT NULL DEFAULT 'default'
);

CREATE TABLE IF NOT EXISTS delivery_queue (
    id INTEGER PRIMARY KEY,
    from_user_id INTEGER NOT NULL REFERENCES users(id),
    recipient TEXT NOT NULL,
    message_path TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL,
    last_error TEXT,
    status TEXT NOT NULL DEFAULT 'pending'
);

CREATE INDEX IF NOT EXISTS idx_delivery_queue_status ON delivery_queue(status, next_attempt_at);
CREATE INDEX IF NOT EXISTS idx_delivery_queue_recipient ON delivery_queue(recipient);

CREATE TABLE IF NOT EXISTS delivery_logs (
    id INTEGER PRIMARY KEY,
    queue_id INTEGER NOT NULL REFERENCES delivery_queue(id),
    status TEXT NOT NULL,
    recipient TEXT NOT NULL,
    error TEXT,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_delivery_logs_created ON delivery_logs(created_at);

CREATE TABLE IF NOT EXISTS filters (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    path TEXT NOT NULL,
    hooks TEXT NOT NULL DEFAULT '[]',
    enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS custom_domains (
    id INTEGER PRIMARY KEY,
    domain TEXT NOT NULL UNIQUE,
    user_id INTEGER NOT NULL REFERENCES users(id),
    verified BOOLEAN NOT NULL DEFAULT 0,
    verification_token TEXT,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_custom_domains_domain ON custom_domains(domain);
CREATE INDEX IF NOT EXISTS idx_custom_domains_user ON custom_domains(user_id);
"#;

// ── Helpers ──────────────────────────────────────────────────────

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Simple v4-ish UUID for session ids.
fn uuid_v4() -> String {
    use std::fmt::Write;
    let bytes: [u8; 16] = rand_bytes();
    let mut s = String::with_capacity(36);
    for (i, b) in bytes.iter().enumerate() {
        if i == 4 || i == 6 || i == 8 || i == 10 {
            s.push('-');
        }
        let _ = write!(s, "{:02x}", b);
    }
    s
}

fn rand_bytes() -> [u8; 16] {
    let mut buf = [0u8; 16];
    getrandom::getrandom(&mut buf).expect("Failed to generate random bytes");
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_db() -> Database {
        Database::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_schema_creates_tables() {
        let db = test_db();
        // Seed config so get_server_config works
        seed_config(&db, "test.com");
        let (domain, _, _) = db.get_server_config().unwrap();
        assert_eq!(domain, "test.com");
    }

    #[test]
    fn test_server_config_seed_and_update() {
        let db = test_db();
        // Seed server_config
        {
            let conn_lock = db.conn.lock().unwrap();
            conn_lock
                .execute(
                    "INSERT OR IGNORE INTO server_config (id, domain, signup_enabled, dkim_selector) VALUES (1, 'test.com', 1, 'default')",
                    [],
                )
                .unwrap();
            drop(conn_lock);
        }

        let (domain, signup, selector) = db.get_server_config().unwrap();
        assert_eq!(domain, "test.com");
        assert!(signup);
        assert_eq!(selector, "default");

        db.update_server_config(Some("new.com"), Some(false), Some("sep2024"))
            .unwrap();
        let (domain, signup, selector) = db.get_server_config().unwrap();
        assert_eq!(domain, "new.com");
        assert!(!signup);
        assert_eq!(selector, "sep2024");
    }

    #[test]
    fn test_user_crud_lifecycle() {
        let db = test_db();
        // Seed config
        seed_config(&db, "test.com");

        // Create user
        let user_id = db.create_user("alice@test.com", "hash123", true).unwrap();
        assert!(user_id > 0);

        // Get by email
        let (id, hash, is_admin, disabled) =
            db.get_user_by_email("alice@test.com").unwrap().unwrap();
        assert_eq!(id, user_id);
        assert_eq!(hash, "hash123");
        assert!(is_admin);
        assert!(!disabled);

        // Get by id
        let (email, is_admin, disabled) = db.get_user_by_id(user_id).unwrap().unwrap();
        assert_eq!(email, "alice@test.com");
        assert!(is_admin);
        assert!(!disabled);

        // List users
        let users = db.list_users().unwrap();
        assert_eq!(users.len(), 1);

        // Disable user
        assert!(db.disable_user(user_id).unwrap());
        let (_, _, disabled) = db.get_user_by_id(user_id).unwrap().unwrap();
        assert!(disabled);

        // Disable again returns false (already disabled)
        assert!(!db.disable_user(user_id).unwrap());

        // Delete user
        assert!(db.delete_user(user_id).unwrap());
        assert!(db.get_user_by_email("alice@test.com").unwrap().is_none());

        // Delete non-existent returns false
        assert!(!db.delete_user(999).unwrap());
    }

    #[test]
    fn test_token_lifecycle() {
        let db = test_db();
        seed_config(&db, "test.com");

        let user_id = db.create_user("bob@test.com", "hash", false).unwrap();

        // Create token
        let token_id = db
            .create_token(user_id, "tok_hash_1", "API Key", "send")
            .unwrap();
        assert!(token_id > 0);

        // List user tokens
        let tokens = db.list_user_tokens(user_id).unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].1, "API Key");
        assert_eq!(tokens[0].2, "send");
        assert!(!tokens[0].5); // not revoked

        // List all tokens
        let all = db.list_all_tokens().unwrap();
        assert_eq!(all.len(), 1);

        // Validate token
        let result = db.validate_token("tok_hash_1").unwrap();
        assert!(result.is_some());
        let (tid, uid) = result.unwrap();
        assert_eq!(tid, token_id);
        assert_eq!(uid, user_id);

        // Validate non-existent token
        assert!(db.validate_token("bad_hash").unwrap().is_none());

        // Revoke token
        assert!(db.revoke_token(token_id).unwrap());
        assert!(db.validate_token("tok_hash_1").unwrap().is_none());

        // Revoke again returns false
        assert!(!db.revoke_token(token_id).unwrap());
    }

    #[test]
    fn test_session_lifecycle() {
        let db = test_db();
        seed_config(&db, "test.com");

        let user_id = db.create_user("carol@test.com", "hash", false).unwrap();

        // Create session
        let session_id = db.create_session(user_id, now() + 3600).unwrap();
        assert!(!session_id.is_empty());

        // Validate session
        let result = db.validate_session(&session_id).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), user_id);

        // Delete session
        assert!(db.delete_session(&session_id).unwrap());
        assert!(db.validate_session(&session_id).unwrap().is_none());

        // Delete non-existent returns false
        assert!(!db.delete_session("nonexistent").unwrap());
    }

    #[test]
    fn test_session_expiry() {
        let db = test_db();
        seed_config(&db, "test.com");

        let user_id = db.create_user("dave@test.com", "hash", false).unwrap();
        let session_id = db.create_session(user_id, now() - 1).unwrap(); // Already expired

        assert!(db.validate_session(&session_id).unwrap().is_none());
    }

    #[test]
    fn test_queue_lifecycle() {
        let db = test_db();
        seed_config(&db, "test.com");

        let user_id = db.create_user("eve@test.com", "hash", false).unwrap();

        // Enqueue messages
        let id1 = db
            .enqueue(user_id, "recipient1@example.com", "/path/to/msg1.eml")
            .unwrap();
        let id2 = db
            .enqueue(user_id, "recipient2@example.com", "/path/to/msg2.eml")
            .unwrap();

        // Get pending
        let pending = db.get_pending_messages(10).unwrap();
        assert_eq!(pending.len(), 2);

        // Mark delivered
        db.mark_delivered(id1).unwrap();

        // Stats
        let (pending_count, delivered, dead) = db.queue_stats().unwrap();
        assert_eq!(pending_count, 1);
        assert_eq!(delivered, 1);
        assert_eq!(dead, 0);

        // Mark failed with exponential backoff
        db.mark_failed(id2, "Connection refused", 5).unwrap();
        // queue_stats counts by status, not by next_attempt_at
        let (pending_count, _, _) = db.queue_stats().unwrap();
        assert_eq!(pending_count, 1); // id2 is still pending (with future next_attempt_at)

        // Fail enough times to dead-letter
        for _ in 1..5 {
            db.mark_failed(id2, "Still failing", 5).unwrap();
        }
        let (_, _, dead) = db.queue_stats().unwrap();
        assert_eq!(dead, 1);

        // List dead letters
        let dead_letters = db.list_dead_letters().unwrap();
        assert_eq!(dead_letters.len(), 1);

        // Retry
        assert!(db.retry_message(id2).unwrap());
        let (pending, _, dead) = db.queue_stats().unwrap();
        assert_eq!(pending, 1); // only id2 is pending again (id1 is delivered)
        assert_eq!(dead, 0);

        // Purge
        assert!(db.purge_message(id1).unwrap());
        assert!(!db.purge_message(999).unwrap());
    }

    #[test]
    fn test_delivery_logs() {
        let db = test_db();
        seed_config(&db, "test.com");

        let user_id = db.create_user("frank@test.com", "hash", false).unwrap();
        let qid = db.enqueue(user_id, "r@example.com", "/path.eml").unwrap();

        db.log_delivery(qid, "delivered", "r@example.com", None)
            .unwrap();
        db.log_delivery(qid, "bounced", "r2@example.com", Some("User unknown"))
            .unwrap();

        let logs = db.get_delivery_logs(10).unwrap();
        assert_eq!(logs.len(), 2);
    }

    #[test]
    fn test_filters_crud() {
        let db = test_db();

        let fid = db
            .create_filter("block-spam", "/filters/block_spam.py", "[\"data\"]")
            .unwrap();
        assert!(fid > 0);

        let filters = db.list_filters().unwrap();
        assert_eq!(filters.len(), 1);

        let f = db.get_filter(fid).unwrap();
        assert_eq!(f.1, "block-spam");
        assert!(f.4); // enabled

        assert!(db.set_filter_enabled(fid, false).unwrap());
        let f = db.get_filter(fid).unwrap();
        assert!(!f.4);

        assert!(db.delete_filter(fid).unwrap());
        assert!(!db.delete_filter(999).unwrap());
    }

    #[test]
    fn test_custom_domains() {
        let db = test_db();
        seed_config(&db, "test.com");

        let user_id = db.create_user("grace@test.com", "hash", false).unwrap();

        // Add domain
        let domain_id = db.add_custom_domain(user_id, "grace.com").unwrap();
        assert!(domain_id > 0);

        // Not verified yet
        assert!(!db.is_domain_allowed("grace.com"));
        assert!(db.get_domain_owner("grace.com").is_none());

        // List domains
        let domains = db.list_custom_domains(user_id).unwrap();
        assert_eq!(domains.len(), 1);
        assert!(!domains[0].2); // not verified

        // Verify
        assert!(db.verify_custom_domain(domain_id).unwrap());
        assert!(db.is_domain_allowed("grace.com"));
        assert_eq!(db.get_domain_owner("grace.com").unwrap(), user_id);

        // Mailbox user for domain
        let mailbox = db
            .get_mailbox_user_for_domain("grace.com", "info@grace.com")
            .unwrap();
        assert_eq!(mailbox, "grace"); // local part of owner's email

        // Configured domain also works
        assert!(db.is_domain_allowed("test.com"));

        // Verified domains list
        let verified = db.list_verified_domains();
        assert_eq!(verified.len(), 1);

        // Delete
        assert!(db.delete_custom_domain(domain_id).unwrap());
        assert!(!db.is_domain_allowed("grace.com"));
    }

    #[test]
    fn test_uuid_format() {
        let id = uuid_v4();
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn test_now_is_reasonable() {
        let t = now();
        assert!(t > 1700000000); // After Nov 2023
        assert!(t < 2000000000); // Before 2033
    }

    /// Helper: seed server_config for tests
    fn seed_config(db: &Database, domain: &str) {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO server_config (id, domain, signup_enabled, dkim_selector) VALUES (1, ?1, 1, 'default')",
            [domain],
        )
        .unwrap();
    }
}
