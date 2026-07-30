"""Database module for stampd-admin.

Provides async SQLite access using aiosqlite.
The database path is configured via STAMPD_DB_PATH environment variable.
"""

import os
import secrets
from collections.abc import AsyncGenerator
from contextlib import asynccontextmanager

import aiosqlite

DB_PATH = os.getenv("STAMPD_DB_PATH", "/var/lib/stampd/stampd.db")


@asynccontextmanager
async def get_db() -> AsyncGenerator[aiosqlite.Connection, None]:
    """Get a database connection."""
    db = await aiosqlite.connect(DB_PATH)
    db.row_factory = aiosqlite.Row
    try:
        yield db
    finally:
        await db.close()


async def run_migrations():
    """Apply lightweight schema migrations for existing databases."""
    async with get_db() as db:
        cursor = await db.execute("PRAGMA table_info(custom_domains)")
        columns = {row["name"] for row in await cursor.fetchall()}
        if "verification_token" not in columns:
            await db.execute(
                "ALTER TABLE custom_domains ADD COLUMN verification_token TEXT"
            )
            await db.commit()


# ── User queries ───────────────────────────────────────────────────

async def list_users():
    """List all users."""
    async with get_db() as db:
        cursor = await db.execute(
            "SELECT id, email, is_admin, created_at, disabled_at FROM users ORDER BY id"
        )
        rows = await cursor.fetchall()
        return [dict(row) for row in rows]


async def get_user(user_id: int):
    """Get a user by ID."""
    async with get_db() as db:
        cursor = await db.execute(
            "SELECT id, email, is_admin, created_at, disabled_at FROM users WHERE id = ?",
            (user_id,),
        )
        row = await cursor.fetchone()
        return dict(row) if row else None


async def get_user_by_email(email: str):
    """Get a user by email."""
    async with get_db() as db:
        cursor = await db.execute(
            "SELECT id, email, is_admin, created_at, disabled_at FROM users WHERE email = ?",
            (email,),
        )
        row = await cursor.fetchone()
        return dict(row) if row else None


async def disable_user(user_id: int) -> bool:
    """Disable a user account."""
    async with get_db() as db:
        cursor = await db.execute(
            "UPDATE users SET disabled_at = strftime('%s', 'now') WHERE id = ? AND disabled_at IS NULL",
            (user_id,),
        )
        await db.commit()
        return cursor.rowcount > 0


async def delete_user(user_id: int) -> bool:
    """Delete a user account."""
    async with get_db() as db:
        # Revoke all tokens first
        await db.execute("UPDATE tokens SET revoked_at = strftime('%s', 'now') WHERE user_id = ?", (user_id,))
        cursor = await db.execute("DELETE FROM users WHERE id = ?", (user_id,))
        await db.commit()
        return cursor.rowcount > 0


# ── Token queries ──────────────────────────────────────────────────

async def list_all_tokens():
    """List all tokens (admin view)."""
    async with get_db() as db:
        cursor = await db.execute(
            """SELECT t.id, t.user_id, u.email, t.label, t.scope, t.created_at, t.revoked_at
               FROM tokens t JOIN users u ON t.user_id = u.id
               ORDER BY t.created_at DESC"""
        )
        rows = await cursor.fetchall()
        return [dict(row) for row in rows]


async def get_token_stats():
    """Get token statistics."""
    async with get_db() as db:
        cursor = await db.execute(
            """SELECT
                 COUNT(*) as total,
                 SUM(CASE WHEN revoked_at IS NULL THEN 1 ELSE 0 END) as active,
                 SUM(CASE WHEN revoked_at IS NOT NULL THEN 1 ELSE 0 END) as revoked
               FROM tokens"""
        )
        row = await cursor.fetchone()
        return dict(row) if row else {"total": 0, "active": 0, "revoked": 0}


async def revoke_token(token_id: int) -> bool:
    """Revoke a token."""
    async with get_db() as db:
        cursor = await db.execute(
            "UPDATE tokens SET revoked_at = strftime('%s', 'now') WHERE id = ? AND revoked_at IS NULL",
            (token_id,),
        )
        await db.commit()
        return cursor.rowcount > 0


# ── Config queries ─────────────────────────────────────────────────

async def get_server_config():
    """Get server configuration."""
    async with get_db() as db:
        cursor = await db.execute("SELECT * FROM server_config WHERE id = 1")
        row = await cursor.fetchone()
        if row:
            return dict(row)
        return {"domain": "localhost", "signup_enabled": 1, "dkim_selector": "default"}


async def update_server_config(updates: dict) -> bool:
    """Update server configuration."""
    if not updates:
        return False
    async with get_db() as db:
        set_clause = ", ".join(f"{k} = ?" for k in updates)
        values = list(updates.values())
        cursor = await db.execute(
            f"UPDATE server_config SET {set_clause} WHERE id = 1",
            values,
        )
        await db.commit()
        return cursor.rowcount > 0


# ── Quota queries ──────────────────────────────────────────────────

async def get_quota_usage():
    """Get quota usage per user."""
    async with get_db() as db:
        cursor = await db.execute(
            """SELECT u.id, u.email,
                      COALESCE(SUM(f.size), 0) as used_bytes,
                      5368709120 as quota_bytes
               FROM users u
               LEFT JOIN (
                 SELECT user_id, size FROM outbox
                 UNION ALL
                 SELECT user_id, size FROM inbox
               ) f ON f.user_id = u.id
               WHERE u.disabled_at IS NULL
               GROUP BY u.id"""
        )
        rows = await cursor.fetchall()
        return [dict(row) for row in rows]


# ── Queue queries ──────────────────────────────────────────────────

async def list_queue_messages(status: str | None = None):
    """List queue messages."""
    async with get_db() as db:
        if status:
            cursor = await db.execute(
                "SELECT * FROM outbox WHERE status = ? ORDER BY created_at DESC LIMIT 100",
                (status,),
            )
        else:
            cursor = await db.execute(
                "SELECT * FROM outbox ORDER BY created_at DESC LIMIT 100"
            )
        rows = await cursor.fetchall()
        return [dict(row) for row in rows]


async def retry_message(msg_id: int) -> bool:
    """Retry a dead-lettered message."""
    async with get_db() as db:
        cursor = await db.execute(
            "UPDATE outbox SET status = 'pending', attempts = 0 WHERE id = ? AND status = 'dead'",
            (msg_id,),
        )
        await db.commit()
        return cursor.rowcount > 0


async def purge_message(msg_id: int) -> bool:
    """Purge a message from the queue."""
    async with get_db() as db:
        cursor = await db.execute("DELETE FROM outbox WHERE id = ?", (msg_id,))
        await db.commit()
        return cursor.rowcount > 0


# ── Delivery log queries ───────────────────────────────────────────

async def get_delivery_logs(status: str | None = None, recipient: str | None = None, limit: int = 50):
    """Get delivery logs with optional filters."""
    async with get_db() as db:
        query = "SELECT * FROM delivery_logs WHERE 1=1"
        params = []
        if status:
            query += " AND status = ?"
            params.append(status)
        if recipient:
            query += " AND recipient LIKE ?"
            params.append(f"%{recipient}%")
        query += " ORDER BY created_at DESC LIMIT ?"
        params.append(limit)
        cursor = await db.execute(query, params)
        rows = await cursor.fetchall()
        return [dict(row) for row in rows]


# ── Filter queries ─────────────────────────────────────────────────

async def list_filters():
    """List all filters."""
    async with get_db() as db:
        cursor = await db.execute("SELECT * FROM filters ORDER BY id")
        rows = await cursor.fetchall()
        return [dict(row) for row in rows]


async def get_filter(filter_id: int):
    """Get a filter by ID."""
    async with get_db() as db:
        cursor = await db.execute("SELECT * FROM filters WHERE id = ?", (filter_id,))
        row = await cursor.fetchone()
        return dict(row) if row else None


async def create_filter(name: str, path: str, hooks: list) -> int:
    """Create a new filter."""
    async with get_db() as db:
        cursor = await db.execute(
            "INSERT INTO filters (name, path, hooks, enabled) VALUES (?, ?, ?, 1)",
            (name, path, ",".join(hooks)),
        )
        await db.commit()
        return cursor.lastrowid


async def set_filter_enabled(filter_id: int, enabled: bool) -> bool:
    """Enable or disable a filter."""
    async with get_db() as db:
        cursor = await db.execute(
            "UPDATE filters SET enabled = ? WHERE id = ?",
            (1 if enabled else 0, filter_id),
        )
        await db.commit()
        return cursor.rowcount > 0


async def delete_filter(filter_id: int) -> bool:
    """Delete a filter."""
    async with get_db() as db:
        cursor = await db.execute("DELETE FROM filters WHERE id = ?", (filter_id,))
        await db.commit()
        return cursor.rowcount > 0


# ── Custom domain queries ─────────────────────────────────────────

async def list_custom_domains(user_id: int | None = None):
    """List custom domains, optionally filtered by user."""
    async with get_db() as db:
        if user_id:
            cursor = await db.execute(
                "SELECT * FROM custom_domains WHERE user_id = ? ORDER BY id",
                (user_id,),
            )
        else:
            cursor = await db.execute("SELECT * FROM custom_domains ORDER BY id")
        rows = await cursor.fetchall()
        return [dict(row) for row in rows]


async def add_custom_domain(user_id: int, domain: str) -> tuple[int, str]:
    """Add a custom domain. Returns (domain_id, verification_token)."""
    token = f"stampd-verify-{secrets.token_hex(16)}"
    async with get_db() as db:
        cursor = await db.execute(
            "INSERT INTO custom_domains (user_id, domain, verified, verification_token) VALUES (?, ?, 0, ?)",
            (user_id, domain.lower(), token),
        )
        await db.commit()
        return cursor.lastrowid, token


async def get_custom_domain(domain_id: int) -> dict | None:
    """Get a custom domain by ID."""
    async with get_db() as db:
        cursor = await db.execute(
            "SELECT * FROM custom_domains WHERE id = ?", (domain_id,)
        )
        row = await cursor.fetchone()
        return dict(row) if row else None


async def verify_custom_domain(domain_id: int) -> bool:
    """Mark a custom domain as verified."""
    async with get_db() as db:
        cursor = await db.execute(
            "UPDATE custom_domains SET verified = 1 WHERE id = ?",
            (domain_id,),
        )
        await db.commit()
        return cursor.rowcount > 0


async def delete_custom_domain(domain_id: int) -> bool:
    """Delete a custom domain."""
    async with get_db() as db:
        cursor = await db.execute("DELETE FROM custom_domains WHERE id = ?", (domain_id,))
        await db.commit()
        return cursor.rowcount > 0


# ── Auth validation queries ─────────────────────────────────────

async def validate_session(session_id: str) -> dict | None:
    """Validate a session and return user info if valid."""
    import time

    now = int(time.time())
    async with get_db() as db:
        cursor = await db.execute(
            """SELECT u.id, u.email, u.is_admin
               FROM sessions s JOIN users u ON s.user_id = u.id
               WHERE s.id = ? AND s.expires_at > ? AND u.disabled_at IS NULL""",
            (session_id, now),
        )
        row = await cursor.fetchone()
        return dict(row) if row else None


async def validate_token_hash(token_hash: str) -> dict | None:
    """Validate a token hash and return user info if valid."""
    async with get_db() as db:
        cursor = await db.execute(
            """SELECT u.id, u.email, u.is_admin
               FROM auth_tokens t JOIN users u ON t.user_id = u.id
               WHERE t.token_hash = ? AND t.revoked_at IS NULL AND u.disabled_at IS NULL""",
            (token_hash,),
        )
        row = await cursor.fetchone()
        return dict(row) if row else None
