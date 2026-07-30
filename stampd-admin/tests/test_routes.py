"""Comprehensive tests for stampd-admin routes."""

import os
import time

import aiosqlite
import pytest
import pytest_asyncio
from httpx import ASGITransport, AsyncClient

from app.main import app  # noqa: E402
from app import database as db  # noqa: E402

TEST_DB = os.environ["STAMPD_DB_PATH"]

# ── Schema ─────────────────────────────────────────────────────────

SCHEMA = """
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL DEFAULT '',
    is_admin BOOLEAN NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    disabled_at INTEGER
);

CREATE TABLE IF NOT EXISTS tokens (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    token_hash TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    scope TEXT NOT NULL DEFAULT 'send',
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    last_used_at INTEGER,
    revoked_at INTEGER
);

CREATE TABLE IF NOT EXISTS auth_tokens (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    token_hash TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    scope TEXT NOT NULL DEFAULT 'send',
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    last_used_at INTEGER,
    revoked_at INTEGER
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    expires_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS server_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    domain TEXT NOT NULL DEFAULT 'localhost',
    signup_enabled BOOLEAN NOT NULL DEFAULT 1,
    dkim_selector TEXT NOT NULL DEFAULT 'default'
);

CREATE TABLE IF NOT EXISTS outbox (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    recipient TEXT NOT NULL DEFAULT '',
    subject TEXT NOT NULL DEFAULT '',
    message_path TEXT NOT NULL DEFAULT '',
    size INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    next_attempt_at INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE IF NOT EXISTS delivery_logs (
    id INTEGER PRIMARY KEY,
    queue_id INTEGER,
    status TEXT NOT NULL,
    recipient TEXT NOT NULL,
    error TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE IF NOT EXISTS filters (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    path TEXT NOT NULL,
    hooks TEXT NOT NULL DEFAULT '[]',
    enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE IF NOT EXISTS custom_domains (
    id INTEGER PRIMARY KEY,
    domain TEXT NOT NULL UNIQUE,
    user_id INTEGER NOT NULL REFERENCES users(id),
    verified BOOLEAN NOT NULL DEFAULT 0,
    verification_token TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

INSERT OR IGNORE INTO server_config (id, domain, signup_enabled, dkim_selector)
VALUES (1, 'localhost', 1, 'default');
"""


async def _setup_db():
    """Create schema in the test database."""
    async with aiosqlite.connect(TEST_DB) as conn:
        await conn.executescript(SCHEMA)
        await conn.commit()


async def _teardown_db():
    """Remove all data from tables."""
    async with aiosqlite.connect(TEST_DB) as conn:
        for table in (
            "custom_domains",
            "delivery_logs",
            "outbox",
            "filters",
            "sessions",
            "auth_tokens",
            "tokens",
            "users",
        ):
            await conn.execute(f"DELETE FROM {table}")
        await conn.execute("UPDATE server_config SET domain='localhost', signup_enabled=1, dkim_selector='default' WHERE id=1")
        await conn.commit()


@pytest_asyncio.fixture(autouse=True)
async def _clean_db():
    """Reset database before each test."""
    await _setup_db()
    yield
    await _teardown_db()


@pytest_asyncio.fixture
async def client():
    """Provide an AsyncClient wired to the FastAPI app."""
    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as c:
        yield c


async def _create_user(email="alice@example.com", is_admin=0):
    async with aiosqlite.connect(TEST_DB) as conn:
        cur = await conn.execute(
            "INSERT INTO users (email, is_admin) VALUES (?, ?)",
            (email, is_admin),
        )
        await conn.commit()
        return cur.lastrowid


async def _create_token(user_id, label="t1", token_hash="hash1", revoked=False):
    async with aiosqlite.connect(TEST_DB) as conn:
        revoked_at = int(time.time()) if revoked else None
        cur = await conn.execute(
            "INSERT INTO tokens (user_id, token_hash, label, revoked_at) VALUES (?, ?, ?, ?)",
            (user_id, token_hash, label, revoked_at),
        )
        await conn.commit()
        return cur.lastrowid


async def _create_auth_token(user_id, token_hash="ahash1"):
    async with aiosqlite.connect(TEST_DB) as conn:
        cur = await conn.execute(
            "INSERT INTO auth_tokens (user_id, token_hash) VALUES (?, ?)",
            (user_id, token_hash),
        )
        await conn.commit()
        return cur.lastrowid


async def _create_session(user_id, session_id="sess1", expires_offset=3600):
    now = int(time.time())
    async with aiosqlite.connect(TEST_DB) as conn:
        await conn.execute(
            "INSERT INTO sessions (id, user_id, expires_at) VALUES (?, ?, ?)",
            (session_id, user_id, now + expires_offset),
        )
        await conn.commit()


async def _create_outbox_msg(user_id, status="pending"):
    async with aiosqlite.connect(TEST_DB) as conn:
        cur = await conn.execute(
            "INSERT INTO outbox (user_id, status, next_attempt_at) VALUES (?, ?, 0)",
            (user_id, status),
        )
        await conn.commit()
        return cur.lastrowid


async def _create_delivery_log(status, recipient="bob@example.com", queue_id=None):
    async with aiosqlite.connect(TEST_DB) as conn:
        cur = await conn.execute(
            "INSERT INTO delivery_logs (queue_id, status, recipient) VALUES (?, ?, ?)",
            (queue_id, status, recipient),
        )
        await conn.commit()
        return cur.lastrowid


async def _create_filter(name="spam", path="/filters/spam.py", hooks="mail_from,rcpt_to"):
    async with aiosqlite.connect(TEST_DB) as conn:
        cur = await conn.execute(
            "INSERT INTO filters (name, path, hooks) VALUES (?, ?, ?)",
            (name, path, hooks),
        )
        await conn.commit()
        return cur.lastrowid


async def _create_domain(user_id, domain="example.com", verified=0, token="stampd-verify-abc"):
    async with aiosqlite.connect(TEST_DB) as conn:
        cur = await conn.execute(
            "INSERT INTO custom_domains (user_id, domain, verified, verification_token) VALUES (?, ?, ?, ?)",
            (user_id, domain, verified, token),
        )
        await conn.commit()
        return cur.lastrowid


# ═══════════════════════════════════════════════════════════════════
# 1. Health endpoint
# ═══════════════════════════════════════════════════════════════════


@pytest.mark.asyncio
async def test_health_ok(client):
    resp = await client.get("/health")
    assert resp.status_code == 200
    body = resp.json()
    assert body["status"] == "ok"
    assert body["service"] == "stampd-admin"


# ═══════════════════════════════════════════════════════════════════
# 2. User CRUD
# ═══════════════════════════════════════════════════════════════════


@pytest.mark.asyncio
async def test_list_users_empty(client):
    resp = await client.get("/admin/users")
    assert resp.status_code == 200
    assert resp.json() == []


@pytest.mark.asyncio
async def test_list_users(client):
    await _create_user("a@test.com")
    await _create_user("b@test.com")
    resp = await client.get("/admin/users")
    assert resp.status_code == 200
    users = resp.json()
    assert len(users) == 2
    assert {u["email"] for u in users} == {"a@test.com", "b@test.com"}


@pytest.mark.asyncio
async def test_get_user(client):
    uid = await _create_user("x@test.com")
    resp = await client.get(f"/admin/users/{uid}")
    assert resp.status_code == 200
    assert resp.json()["email"] == "x@test.com"


@pytest.mark.asyncio
async def test_get_user_not_found(client):
    resp = await client.get("/admin/users/9999")
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_disable_user(client):
    uid = await _create_user("dis@test.com")
    resp = await client.patch(f"/admin/users/{uid}/disable")
    assert resp.status_code == 200
    assert resp.json()["ok"] is True
    # Confirm disabled_at is set
    user = await db.get_user(uid)
    assert user["disabled_at"] is not None


@pytest.mark.asyncio
async def test_disable_user_already_disabled(client):
    uid = await _create_user("dup@test.com")
    await db.disable_user(uid)
    resp = await client.patch(f"/admin/users/{uid}/disable")
    assert resp.status_code == 400
    assert "already disabled" in resp.json()["detail"]


@pytest.mark.asyncio
async def test_disable_user_not_found(client):
    resp = await client.patch("/admin/users/9999/disable")
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_delete_user(client):
    uid = await _create_user("del@test.com")
    resp = await client.delete(f"/admin/users/{uid}")
    assert resp.status_code == 200
    assert resp.json()["ok"] is True
    assert await db.get_user(uid) is None


@pytest.mark.asyncio
async def test_delete_user_not_found(client):
    resp = await client.delete("/admin/users/9999")
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_delete_user_revokes_tokens(client):
    uid = await _create_user("tok@test.com")
    await _create_token(uid, label="t1", token_hash="h1")
    await _create_token(uid, label="t2", token_hash="h2")
    await client.delete(f"/admin/users/{uid}")
    tokens = await db.list_all_tokens()
    assert all(t["revoked_at"] is not None for t in tokens)


@pytest.mark.asyncio
async def test_get_user_tokens(client):
    uid = await _create_user("ut@test.com")
    await _create_token(uid, label="t1", token_hash="h1")
    await _create_token(uid, label="t2", token_hash="h2")
    resp = await client.get(f"/admin/users/{uid}/tokens")
    assert resp.status_code == 200
    assert len(resp.json()) == 2


@pytest.mark.asyncio
async def test_get_user_tokens_not_found(client):
    resp = await client.get("/admin/users/9999/tokens")
    assert resp.status_code == 404


# ═══════════════════════════════════════════════════════════════════
# 3. Token management
# ═══════════════════════════════════════════════════════════════════


@pytest.mark.asyncio
async def test_list_tokens_empty(client):
    resp = await client.get("/admin/tokens")
    assert resp.status_code == 200
    assert resp.json() == []


@pytest.mark.asyncio
async def test_list_tokens(client):
    uid = await _create_user("tl@test.com")
    await _create_token(uid, label="t1", token_hash="h1")
    await _create_token(uid, label="t2", token_hash="h2", revoked=True)
    resp = await client.get("/admin/tokens")
    assert resp.status_code == 200
    tokens = resp.json()
    assert len(tokens) == 2
    assert all(t["email"] == "tl@test.com" for t in tokens)


@pytest.mark.asyncio
async def test_token_stats(client):
    uid = await _create_user("ts@test.com")
    await _create_token(uid, token_hash="h1")
    await _create_token(uid, token_hash="h2")
    await _create_token(uid, token_hash="h3", revoked=True)
    resp = await client.get("/admin/tokens/stats")
    assert resp.status_code == 200
    stats = resp.json()
    assert stats["total"] == 3
    assert stats["active"] == 2
    assert stats["revoked"] == 1


@pytest.mark.asyncio
async def test_revoke_token(client):
    uid = await _create_user("rv@test.com")
    tid = await _create_token(uid, token_hash="h1")
    resp = await client.delete(f"/admin/tokens/{tid}")
    assert resp.status_code == 200
    assert resp.json()["ok"] is True


@pytest.mark.asyncio
async def test_revoke_token_not_found(client):
    resp = await client.delete("/admin/tokens/9999")
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_revoke_already_revoked(client):
    uid = await _create_user("rv2@test.com")
    tid = await _create_token(uid, token_hash="h1", revoked=True)
    resp = await client.delete(f"/admin/tokens/{tid}")
    assert resp.status_code == 404


# ═══════════════════════════════════════════════════════════════════
# 4. Config
# ═══════════════════════════════════════════════════════════════════


@pytest.mark.asyncio
async def test_get_config(client):
    resp = await client.get("/admin/config")
    assert resp.status_code == 200
    cfg = resp.json()
    assert cfg["domain"] == "localhost"
    assert cfg["signup_enabled"] == 1


@pytest.mark.asyncio
async def test_update_config_domain(client):
    resp = await client.patch("/admin/config", json={"domain": "mail.test.com"})
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is True
    assert body["config"]["domain"] == "mail.test.com"


@pytest.mark.asyncio
async def test_update_config_signup_toggle(client):
    resp = await client.patch("/admin/config", json={"signup_enabled": False})
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is True
    assert body["config"]["signup_enabled"] == 0


@pytest.mark.asyncio
async def test_update_config_dkim_selector(client):
    resp = await client.patch("/admin/config", json={"dkim_selector": "selector2"})
    assert resp.status_code == 200
    body = resp.json()
    assert body["config"]["dkim_selector"] == "selector2"


@pytest.mark.asyncio
async def test_update_config_no_fields(client):
    resp = await client.patch("/admin/config", json={})
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is False
    assert "No fields" in body["error"]


@pytest.mark.asyncio
async def test_update_config_empty_values_filtered(client):
    resp = await client.patch("/admin/config", json={"domain": None})
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is False


# ═══════════════════════════════════════════════════════════════════
# 5. Queue
# ═══════════════════════════════════════════════════════════════════


@pytest.mark.asyncio
async def test_list_queue_empty(client):
    resp = await client.get("/admin/queue")
    assert resp.status_code == 200
    assert resp.json() == []


@pytest.mark.asyncio
async def test_list_queue(client):
    uid = await _create_user("q@test.com")
    await _create_outbox_msg(uid, "pending")
    await _create_outbox_msg(uid, "dead")
    resp = await client.get("/admin/queue")
    assert resp.status_code == 200
    assert len(resp.json()) == 2


@pytest.mark.asyncio
async def test_list_queue_filter_status(client):
    uid = await _create_user("qf@test.com")
    await _create_outbox_msg(uid, "pending")
    await _create_outbox_msg(uid, "dead")
    resp = await client.get("/admin/queue", params={"status": "dead"})
    assert resp.status_code == 200
    msgs = resp.json()
    assert len(msgs) == 1
    assert msgs[0]["status"] == "dead"


@pytest.mark.asyncio
async def test_retry_message(client):
    uid = await _create_user("qr@test.com")
    mid = await _create_outbox_msg(uid, "dead")
    resp = await client.post(f"/admin/queue/{mid}/retry")
    assert resp.status_code == 200
    assert resp.json()["ok"] is True


@pytest.mark.asyncio
async def test_retry_message_not_dead(client):
    uid = await _create_user("qrd@test.com")
    mid = await _create_outbox_msg(uid, "pending")
    resp = await client.post(f"/admin/queue/{mid}/retry")
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_retry_message_not_found(client):
    resp = await client.post("/admin/queue/9999/retry")
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_purge_message(client):
    uid = await _create_user("qp@test.com")
    mid = await _create_outbox_msg(uid, "pending")
    resp = await client.delete(f"/admin/queue/{mid}")
    assert resp.status_code == 200
    assert resp.json()["ok"] is True
    # Confirm deleted
    async with aiosqlite.connect(TEST_DB) as conn:
        cur = await conn.execute("SELECT COUNT(*) FROM outbox WHERE id=?", (mid,))
        assert (await cur.fetchone())[0] == 0


@pytest.mark.asyncio
async def test_purge_message_not_found(client):
    resp = await client.delete("/admin/queue/9999")
    assert resp.status_code == 404


# ═══════════════════════════════════════════════════════════════════
# 6. Logs
# ═══════════════════════════════════════════════════════════════════


@pytest.mark.asyncio
async def test_get_logs_empty(client):
    resp = await client.get("/admin/logs")
    assert resp.status_code == 200
    assert resp.json() == []


@pytest.mark.asyncio
async def test_get_logs(client):
    await _create_delivery_log("sent", "a@b.com")
    await _create_delivery_log("failed", "c@d.com")
    resp = await client.get("/admin/logs")
    assert resp.status_code == 200
    assert len(resp.json()) == 2


@pytest.mark.asyncio
async def test_get_logs_filter_status(client):
    await _create_delivery_log("sent", "a@b.com")
    await _create_delivery_log("failed", "c@d.com")
    resp = await client.get("/admin/logs", params={"status": "sent"})
    assert resp.status_code == 200
    assert len(resp.json()) == 1


@pytest.mark.asyncio
async def test_get_logs_filter_recipient(client):
    await _create_delivery_log("sent", "alice@test.com")
    await _create_delivery_log("sent", "bob@test.com")
    resp = await client.get("/admin/logs", params={"recipient": "alice"})
    assert resp.status_code == 200
    logs = resp.json()
    assert len(logs) == 1
    assert logs[0]["recipient"] == "alice@test.com"


@pytest.mark.asyncio
async def test_get_logs_limit(client):
    for i in range(5):
        await _create_delivery_log("sent", f"user{i}@test.com")
    resp = await client.get("/admin/logs", params={"limit": 3})
    assert resp.status_code == 200
    assert len(resp.json()) == 3


# ═══════════════════════════════════════════════════════════════════
# 7. Filters
# ═══════════════════════════════════════════════════════════════════


@pytest.mark.asyncio
async def test_list_filters_empty(client):
    resp = await client.get("/admin/filters")
    assert resp.status_code == 200
    assert resp.json() == []


@pytest.mark.asyncio
async def test_list_filters(client):
    await _create_filter("f1", "/f1.py", "mail_from")
    await _create_filter("f2", "/f2.py", "rcpt_to")
    resp = await client.get("/admin/filters")
    assert resp.status_code == 200
    assert len(resp.json()) == 2


@pytest.mark.asyncio
async def test_get_filter(client):
    fid = await _create_filter("f1", "/f1.py", "mail_from")
    resp = await client.get(f"/admin/filters/{fid}")
    assert resp.status_code == 200
    assert resp.json()["name"] == "f1"


@pytest.mark.asyncio
async def test_get_filter_not_found(client):
    resp = await client.get("/admin/filters/9999")
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_create_filter(client):
    resp = await client.post(
        "/admin/filters",
        json={"name": "spam", "path": "/spam.py", "hooks": ["mail_from", "rcpt_to"]},
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is True
    assert body["filter"]["name"] == "spam"
    assert body["filter"]["enabled"] == 1


@pytest.mark.asyncio
async def test_create_filter_invalid_hook(client):
    resp = await client.post(
        "/admin/filters",
        json={"name": "bad", "path": "/bad.py", "hooks": ["invalid_hook"]},
    )
    assert resp.status_code == 400
    assert "Invalid hook" in resp.json()["detail"]


@pytest.mark.asyncio
async def test_update_filter_enable_disable(client):
    fid = await _create_filter("f1", "/f1.py", "data")
    resp = await client.patch(f"/admin/filters/{fid}", json={"enabled": False})
    assert resp.status_code == 200
    assert resp.json()["ok"] is True
    assert resp.json()["filter"]["enabled"] == 0

    resp = await client.patch(f"/admin/filters/{fid}", json={"enabled": True})
    assert resp.status_code == 200
    assert resp.json()["filter"]["enabled"] == 1


@pytest.mark.asyncio
async def test_update_filter_not_found(client):
    resp = await client.patch("/admin/filters/9999", json={"enabled": True})
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_delete_filter(client):
    fid = await _create_filter("f1", "/f1.py", "data")
    resp = await client.delete(f"/admin/filters/{fid}")
    assert resp.status_code == 200
    assert resp.json()["ok"] is True


@pytest.mark.asyncio
async def test_delete_filter_not_found(client):
    resp = await client.delete("/admin/filters/9999")
    assert resp.status_code == 404


# ═══════════════════════════════════════════════════════════════════
# 8. Auth validation
# ═══════════════════════════════════════════════════════════════════


@pytest.mark.asyncio
async def test_validate_session_valid(client):
    uid = await _create_user("s@test.com", is_admin=1)
    await _create_session(uid, session_id="sess-valid")
    resp = await client.post("/auth/validate", json={"session_id": "sess-valid"})
    assert resp.status_code == 200
    body = resp.json()
    assert body["valid"] is True
    assert body["user_id"] == uid
    assert body["is_admin"] is True


@pytest.mark.asyncio
async def test_validate_session_invalid(client):
    resp = await client.post("/auth/validate", json={"session_id": "nonexistent"})
    assert resp.status_code == 200
    body = resp.json()
    assert body["valid"] is False


@pytest.mark.asyncio
async def test_validate_session_expired(client):
    uid = await _create_user("se@test.com")
    await _create_session(uid, session_id="sess-exp", expires_offset=-3600)
    resp = await client.post("/auth/validate", json={"session_id": "sess-exp"})
    assert resp.status_code == 200
    assert resp.json()["valid"] is False


@pytest.mark.asyncio
async def test_validate_session_disabled_user(client):
    uid = await _create_user("sd@test.com")
    await _create_session(uid, session_id="sess-dis")
    await db.disable_user(uid)
    resp = await client.post("/auth/validate", json={"session_id": "sess-dis"})
    assert resp.status_code == 200
    assert resp.json()["valid"] is False


@pytest.mark.asyncio
async def test_validate_token_hash_valid(client):
    uid = await _create_user("th@test.com", is_admin=1)
    await _create_auth_token(uid, token_hash="good-hash")
    resp = await client.post("/auth/validate", json={"token_hash": "good-hash"})
    assert resp.status_code == 200
    body = resp.json()
    assert body["valid"] is True
    assert body["user_id"] == uid
    assert body["is_admin"] is True


@pytest.mark.asyncio
async def test_validate_token_hash_invalid(client):
    resp = await client.post("/auth/validate", json={"token_hash": "nope"})
    assert resp.status_code == 200
    assert resp.json()["valid"] is False


@pytest.mark.asyncio
async def test_validate_no_credential(client):
    resp = await client.post("/auth/validate", json={})
    assert resp.status_code == 200
    assert resp.json()["valid"] is False


# ═══════════════════════════════════════════════════════════════════
# 9. Domain management
# ═══════════════════════════════════════════════════════════════════


@pytest.mark.asyncio
async def test_list_domains_empty(client):
    resp = await client.get("/admin/domains")
    assert resp.status_code == 200
    assert resp.json() == []


@pytest.mark.asyncio
async def test_list_domains(client):
    uid = await _create_user("d@test.com")
    await _create_domain(uid, "example.com")
    await _create_domain(uid, "test.org")
    resp = await client.get("/admin/domains")
    assert resp.status_code == 200
    assert len(resp.json()) == 2


@pytest.mark.asyncio
async def test_list_domains_filter_by_user(client):
    u1 = await _create_user("d1@test.com")
    u2 = await _create_user("d2@test.com")
    await _create_domain(u1, "a.com")
    await _create_domain(u2, "b.com")
    resp = await client.get("/admin/domains", params={"user_id": u1})
    assert resp.status_code == 200
    domains = resp.json()
    assert len(domains) == 1
    assert domains[0]["domain"] == "a.com"


@pytest.mark.asyncio
async def test_add_domain(client):
    resp = await client.post("/admin/domains", json={"domain": "mydomain.com"})
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is True
    assert body["domain"]["domain"] == "mydomain.com"
    assert body["domain"]["verified"] is False
    assert "dns_instructions" in body
    assert body["dns_instructions"]["record_type"] == "TXT"


@pytest.mark.asyncio
async def test_add_domain_invalid(client):
    resp = await client.post("/admin/domains", json={"domain": "nodot"})
    assert resp.status_code == 400
    assert "Valid domain required" in resp.json()["detail"]


@pytest.mark.asyncio
async def test_verify_domain_not_found(client):
    resp = await client.post("/admin/domains/verify", json={"id": 9999})
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_verify_domain_already_verified(client):
    uid = await _create_user("dv@test.com")
    did = await _create_domain(uid, "verified.com", verified=1)
    resp = await client.post("/admin/domains/verify", json={"id": did})
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is True
    assert body["verified"] is True
    assert "already verified" in body["message"]


@pytest.mark.asyncio
async def test_verify_domain_not_found_in_dns(client):
    """Verify returns ok=False when DNS TXT record is missing."""
    uid = await _create_user("dn@test.com")
    did = await _create_domain(uid, "nodns.example.com", token="stampd-verify-nodns")
    resp = await client.post("/admin/domains/verify", json={"id": did})
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is False
    assert body["verified"] is False
    assert "instructions" in body


@pytest.mark.asyncio
async def test_delete_domain(client):
    uid = await _create_user("dd@test.com")
    did = await _create_domain(uid, "del.example.com")
    resp = await client.delete(f"/admin/domains/{did}")
    assert resp.status_code == 200
    assert resp.json()["ok"] is True
    assert await db.get_custom_domain(did) is None


@pytest.mark.asyncio
async def test_delete_domain_not_found(client):
    resp = await client.delete("/admin/domains/9999")
    assert resp.status_code == 404
