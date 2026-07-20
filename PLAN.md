# Stampd — Development Plan (v0.1.0 → v1.0.0)

**Based on:** spec.md (locked)
**Status:** Active development plan
**Start date:** July 2026

---

## Phase Overview

| Phase | Version Range | Focus | Key Deliverable |
|-------|---------------|-------|-----------------|
| **0.1.x** | 0.1.0–0.1.5 | Foundation | Config, CLI, project scaffolding |
| **0.2.x** | 0.2.0–0.2.8 | Engine Core | Inbound SMTP, Maildir, queue |
| **0.3.x** | 0.3.0–0.3.6 | Gateway | API surface, auth, OpenAPI spec |
| **0.4.x** | 0.4.0–0.4.5 | Admin Service | User/token management, domain config |
| **0.5.x** | 0.5.0–0.5.4 | Web UI | Reference implementation |
| **0.6.x** | 0.6.0–0.6.3 | Filters | Transit hooks, user scripts |
| **0.7.x** | 0.7.0–0.7.3 | Integration | End-to-end flow, multi-service |
| **0.8.x** | 0.8.0–0.8.4 | Hardening | Testing, security, performance |
| **0.9.x** | 0.9.0–0.9.2 | Documentation | Docs, examples, deployment guide |
| **1.0.0** | 1.0.0 | Release | Production-ready v1 |

---

## Phase 0.1.x — Foundation (Weeks 1–2)

**Goal:** Project exists, compiles, can be started, config loads.

### 0.1.0 — Project Scaffolding
- [ ] Initialize workspace root (Cargo workspace for Rust)
- [ ] Create directory structure per spec §4
- [ ] Add `stampd.toml` config file with schema
- [ ] Implement config parser (TOML → typed structs)
- [ ] Create `docs/failure-modes.md` (spec §9)
- [ ] Set up CI basics (cargo check, clippy, test)

### 0.1.1 — CLI Skeleton
- [ ] Create `stampd-cli` crate
- [ ] Implement `stampd up` command (reads config, spawns enabled services)
- [ ] Implement `stampd status` command
- [ ] Implement `stampd up --only engine,gateway` filter
- [ ] Process supervision: spawn, monitor, restart-on-crash with backoff
- [ ] Log tagging by service name

### 0.1.2 — Engine Crate Scaffold
- [ ] Create `stampd-engine` crate with binary target
- [ ] Stub `smtpd` module (port 25, accepts connections, logs)
- [ ] Stub `submissiond` module (port 587, accepts connections, logs)
- [ ] Stub `maildir` module (create mailbox dirs on startup)
- [ ] Stub `queue` module (empty delivery queue)

### 0.1.3 — Gateway Crate Scaffold
- [ ] Create `stampd-gateway` package (Node/TS, Fastify)
- [ ] Health check endpoint: `GET /health`
- [ ] Config validation on startup
- [ ] Structured logging (pino)

### 0.1.4 — Admin Crate Scaffold
- [ ] Create `stampd-admin` package (Python, FastAPI)
- [ ] Health check endpoint: `GET /health`
- [ ] SQLite connection pool setup
- [ ] Config validation on startup

### 0.1.5 — Phase 0.1 Validation
- [ ] `stampd up` starts all enabled services
- [ ] `stampd status` shows running services + ports
- [ ] Each service responds on `/health`
- [ ] Config hot-reload not required (restart only)
- [ ] Manual: `curl localhost:8080/health` → 200

**Exit criteria:** CLI can start/stop services, all scaffolds compile and run.

---

## Phase 0.2.x — Engine Core (Weeks 3–6)

**Goal:** Inbound mail received and stored, outbound mail sent and queued.

### 0.2.0 — SQLite Schema
- [ ] Create migration system (sqlx or rusqlite migrations)
- [ ] Implement all tables from spec §10:
  - `users`, `auth_tokens`, `sessions`, `server_config`, `delivery_queue`
- [ ] Seed `server_config` with initial domain
- [ ] Add indexes for common queries

### 0.2.1 — Inbound SMTP Parser
- [ ] RFC 5321 command parser (HELO/EHLO, MAIL FROM, RCPT TO, DATA, RSET, QUIT)
- [ ] Handle multi-line responses
- [ ] Reject malformed commands with proper SMTP codes
- [ ] Log all commands with timestamps

### 0.2.2 — RCPT TO Validation
- [ ] Load domain from `server_config`
- [ ] Reject `RCPT TO` not matching configured domain
- [ ] Return 550 "User not local" for invalid recipients
- [ ] Track accepted recipients per session

### 0.2.3 — STARTTLS Implementation
- [ ] Offer STARTTLS after EHLO (RFC 3207)
- [ ] Load TLS certificate/key from config
- [ ] Upgrade connection on STARTTLS command
- [ ] Reject plaintext after upgrade failure
- [ ] Log TLS status per session

### 0.2.4 — DATA Command & Maildir Storage
- [ ] Accept DATA after valid RCPT TO
- [ ] Read message body until `\r\n.\r\n`
- [ ] Generate unique Maildir filename (timestamp.pid.hostname)
- [ ] Write to `maildir_path/{domain}/{user}/new/`
- [ ] Return 250 "OK" on success
- [ ] Handle disk errors gracefully

### 0.2.5 — Inbound SPF Check (Best-Effort)
- [ ] Parse sender domain from MAIL FROM
- [ ] DNS lookup for SPF record
- [ ] Validate sending IP against SPF
- [ ] Log SPF result (pass/fail/softfail)
- [ ] Don't reject on softfail (best-effort per spec)

### 0.2.6 — Outbound SMTP Client
- [ ] MX lookup for recipient domain
- [ ] Connect to MX servers (port 25)
- [ ] EHLO handshake
- [ ] STARTTLS if available
- [ ] Send envelope + message
- [ ] Handle 2xx/4xx/5xx responses

### 0.2.7 — DKIM Signing
- [ ] Generate DKIM key pair (RSA 2048+)
- [ ] Store key in config directory
- [ ] Sign outgoing messages (RFC 6376)
- [ ] Add DKIM-Signature header
- [ ] Support key rotation (multiple selectors)

### 0.2.8 — Delivery Queue & Retry
- [ ] Enqueue outbound messages to `delivery_queue`
- [ ] Worker thread processes queue
- [ ] Exponential backoff on failure
- [ ] Dead-letter after N attempts (configurable)
- [ ] Manual retry/purge via admin API

### 0.2.9 — Phase 0.2 Validation
- [ ] Send test email from Gmail → Stampd (inbound)
- [ ] Verify Maildir storage
- [ ] Send test email from Stampd → Gmail (outbound)
- [ ] Verify DKIM signature passes
- [ ] Verify delivery queue handles failures

**Exit criteria:** Bidirectional email flow works end-to-end.

---

## Phase 0.3.x — Gateway API (Weeks 7–9)

**Goal:** Public API surface complete, auth works, OpenAPI spec finalized.

### 0.3.0 — OpenAPI Spec First
- [ ] Write OpenAPI 3.1 spec (all endpoints)
- [ ] Endpoints per spec §6:
  - Auth: login, signup, token management
  - Mailbox: list messages, read message
  - Send: compose, send
  - Admin: user/token/config management
- [ ] Generate docs from spec
- [ ] Set up spec validation in gateway

### 0.3.1 — Auth Middleware
- [ ] Session-based auth (cookie)
- [ ] Token-based auth (Bearer)
- [ ] Rate limiting per IP/user (spec §7)
- [ ] Request validation (schema-based)
- [ ] CORS configuration (spec §5a)

### 0.3.2 — Auth Endpoints
- [ ] `POST /auth/signup` — self-signup (check `signup_enabled`)
- [ ] `POST /auth/login` — email + password → session
- [ ] `POST /auth/logout` — destroy session
- [ ] `POST /auth/tokens` — create send-only token
- [ ] `DELETE /auth/tokens/:id` — revoke token

### 0.3.3 — Mailbox Endpoints
- [ ] `GET /mailbox/messages` — list messages (paginated)
- [ ] `GET /mailbox/messages/:id` — read message (returns .eml)
- [ ] `DELETE /mailbox/messages/:id` — delete message
- [ ] `GET /mailbox/stats` — unread count, quota usage

### 0.3.4 — Send Endpoints
- [ ] `POST /messages/send` — send email (from, to, subject, body)
- [ ] `POST /messages/draft` — save draft (optional)
- [ ] Validate recipient domain (must be external)
- [ ] Enqueue to delivery queue

### 0.3.5 — Transit Integration (Engine ↔ Gateway)
- [ ] Gateway → Engine: queue status, config reload
- [ ] Engine → Gateway: delivery notifications
- [ ] Timeout handling for Transit calls

### 0.3.6 — Phase 0.3 Validation
- [ ] Run OpenAPI spec validator
- [ ] All endpoints return correct status codes
- [ ] Auth blocks unauthorized requests
- [ ] Rate limiting works
- [ ] Manual: signup → login → send email → read mailbox

**Exit criteria:** API is feature-complete, OpenAPI spec is source of truth.

---

## Phase 0.4.x — Admin Service (Weeks 10–11)

**Goal:** Admin controls work, business logic complete.

### 0.4.0 — User Management
- [ ] `GET /admin/users` — list all users (admin only)
- [ ] `PATCH /admin/users/:id/disable` — disable user
- [ ] `DELETE /admin/users/:id` — delete user (cascading)
- [ ] `GET /admin/users/:id/tokens` — view user's tokens

### 0.4.1 — Token Management
- [ ] `GET /admin/tokens` — list all tokens (admin only)
- [ ] `DELETE /admin/tokens/:id` — revoke any user's token
- [ ] `GET /admin/tokens/stats` — active/revoked counts

### 0.4.2 — Domain Config
- [ ] `GET /admin/config` — get server config
- [ ] `PATCH /admin/config` — update domain, DKIM selector
- [ ] `POST /admin/config/dkim/rotate` — rotate DKIM keys
- [ ] Notify engine of config changes via Transit

### 0.4.3 — Quota Management
- [ ] Track mailbox size per user
- [ ] Enforce `default_quota_mb` limit
- [ ] `GET /admin/quota` — list users with quota usage
- [ ] Block send when quota exceeded

### 0.4.4 — Queue Visibility
- [ ] `GET /admin/queue` — list pending/dead-lettered messages
- [ ] `POST /admin/queue/:id/retry` — manual retry
- [ ] `DELETE /admin/queue/:id` — purge message

### 0.4.5 — Delivery Logs
- [ ] `GET /admin/logs` — recent delivery events
- [ ] Filter by status (accepted/rejected/bounced)
- [ ] Filter by user, recipient, date range

### 0.4.6 — Transit Integration (Admin ↔ Engine)
- [ ] Admin → Engine: config changes, user disable
- [ ] Engine → Admin: delivery events
- [ ] Timeout handling

### 0.4.7 — Phase 0.4 Validation
- [ ] Admin can list/disable/delete users
- [ ] Admin can manage tokens
- [ ] Quota enforcement works
- [ ] Delivery logs capture events
- [ ] Manual: create user → send mail → check logs

**Exit criteria:** Admin controls are functional, business logic complete.

---

## Phase 0.5.x — Web UI (Weeks 12–13)

**Goal:** Reference UI proves API is sufficient.

### 0.5.0 — Web UI Scaffold
- [ ] Astro + React setup
- [ ] Design system (minimal, functional)
- [ ] Routing structure
- [ ] API client (generated from OpenAPI spec)

### 0.5.1 — Auth Pages
- [ ] Login page
- [ ] Signup page
- [ ] Token management page

### 0.5.2 — Mailbox Pages
- [ ] Message list (inbox)
- [ ] Message detail view
- [ ] Compose/send form
- [ ] Quota indicator

### 0.5.3 — Admin Pages (if admin user)
- [ ] User management dashboard
- [ ] Token management dashboard
- [ ] Server config page
- [ ] Queue management
- [ ] Delivery logs viewer

### 0.5.4 — Phase 0.5 Validation
- [ ] Full user flow: signup → login → send → receive
- [ ] Admin flow: manage users → view logs
- [ ] No direct database access (all via API)
- [ ] Manual: deploy as separate service, verify it works

**Exit criteria:** Web UI is functional, proves API completeness.

---

## Phase 0.6.x — Filters (Weeks 14–15)

**Goal:** User-defined hooks work, Transit integration proven.

### 0.6.0 — Filter Hook System
- [ ] Define hook interface (MAIL FROM, RCPT TO, DATA)
- [ ] Transit-based invocation from engine
- [ ] Timeout enforcement (`filters.timeout_ms`)
- [ ] Hook result: accept/reject/modify

### 0.6.1 — Filter SDK (Python)
- [ ] Python package for writing filters
- [ ] Decorator/function interface
- [ ] Context object (message, sender, recipients)

### 0.6.2 — Filter SDK (JavaScript)
- [ ] JS/TS package for writing filters
- [ ] Async/await interface
- [ ] Context object

### 0.6.3 — Filter Management
- [ ] Admin API: list/enable/disable filters
- [ ] Filter configuration in `stampd.toml`
- [ ] Filter logs in delivery events

### 0.6.4 — Phase 0.6 Validation
- [ ] Write test filter (Python)
- [ ] Hook called on inbound mail
- [ ] Filter can reject mail
- [ ] Timeout kills hung filter
- [ ] Manual: write filter → send mail → verify filter ran

**Exit criteria:** Filters work, Transit integration proven.

---

## Phase 0.7.x — Integration (Weeks 16–17)

**Goal:** All services work together, end-to-end flows complete.

### 0.7.0 — Transit Full Integration
- [ ] Engine ↔ Gateway: queue status, config reload
- [ ] Engine ↔ Admin: delivery events, user management
- [ ] Engine ↔ Filters: hook invocation
- [ ] All internal calls use Transit (not HTTP)

### 0.7.1 — End-to-End Flow: Inbound
- [ ] External → SMTP → Engine → Maildir
- [ ] Gateway serves mailbox via API
- [ ] Web UI displays message

### 0.7.2 — End-to-End Flow: Outbound
- [ ] Web UI → Gateway → Engine → Queue → External
- [ ] DKIM signing verified by recipient
- [ ] Delivery logs capture event

### 0.7.3 — Failure Mode Validation
- [ ] Admin down → engine still receives mail
- [ ] Gateway down → engine/admin unaffected
- [ ] Web down → zero impact on mail flow
- [ ] Filter timeout → engine continues

### 0.7.4 — Multi-User Testing
- [ ] Multiple users signup
- [ ] Send between users
- [ ] Quota enforcement per user
- [ ] Token management per user

### 0.7.5 — Phase 0.7 Validation
- [ ] All integration tests pass
- [ ] All failure modes verified
- [ ] Multi-user scenario works
- [ ] Manual: full day of usage without crashes

**Exit criteria:** System is functionally complete.

---

## Phase 0.8.x — Hardening (Weeks 18–19)

**Goal:** Production-ready quality.

### 0.8.0 — Security Audit
- [ ] SQL injection testing
- [ ] Auth bypass testing
- [ ] Input validation (malformed SMTP commands)
- [ ] TLS configuration review
- [ ] DKIM key security

### 0.8.1 — Performance Testing
- [ ] Maildir write performance (concurrent users)
- [ ] Queue processing throughput
- [ ] API response times
- [ ] Memory usage under load

### 0.8.2 — Reliability Testing
- [ ] Crash recovery (engine, gateway, admin)
- [ ] Disk full handling
- [ ] Network partition handling
- [ ] Graceful shutdown

### 0.8.3 — Edge Cases
- [ ] Malformed email addresses
- [ ] Very large messages (10MB+)
- [ ] Binary content in messages
- [ ] Special characters in subjects

### 0.8.4 — Phase 0.8 Validation
- [ ] All security tests pass
- [ ] Performance meets targets (100+ concurrent users)
- [ ] Crash recovery works
- [ ] Edge cases handled gracefully

**Exit criteria:** System is hardened and reliable.

---

## Phase 0.9.x — Documentation (Weeks 20–21)

**Goal:** Docs complete, deployment guide ready.

### 0.9.0 — API Documentation
- [ ] OpenAPI spec finalized
- [ ] Generated docs published
- [ ] Authentication guide
- [ ] Error code reference

### 0.9.1 — Deployment Guide
- [ ] systemd unit file
- [ ] Caddy reverse proxy config
- [ ] DNS setup (MX, rDNS, DKIM)
- [ ] TLS certificate setup

### 0.9.2 — User Guide
- [ ] Installation
- [ ] Configuration (`stampd.toml` reference)
- [ ] Web UI usage
- [ ] API usage examples

### 0.9.3 — Developer Guide
- [ ] Contributing guide
- [ ] Architecture overview
- [ ] Filter development
- [ ] Custom UI development

### 0.9.4 — Phase 0.9 Validation
- [ ] All docs reviewed
- [ ] Deployment guide tested on fresh server
- [ ] API docs match implementation
- [ ] No broken links

**Exit criteria:** Documentation is complete and accurate.

---

## Phase 1.0.0 — Release (Week 22)

**Goal:** Production-ready v1 release.

### 1.0.0 — Release Checklist
- [ ] All phase exit criteria met
- [ ] All TODO items resolved
- [ ] Version tagged in git
- [ ] Release notes written
- [ ] Binary packages built (Linux x64, arm64)
- [ ] Docker image published
- [ ] Installation script tested
- [ ] Example `stampd.toml` provided
- [ ] License file added
- [ ] README with quick start

### Post-1.0.0 (v2+)
- [ ] v2: Proton-style encryption at rest
- [ ] v3: Expanded admin controls
- [ ] v4: Hosted multi-tenant service

---

## Dependencies & Critical Path

```
Phase 0.1 (Foundation)
    ↓
Phase 0.2 (Engine Core) ─── depends on 0.1
    ↓
Phase 0.3 (Gateway) ─────── depends on 0.2 (needs working engine)
    ↓
Phase 0.4 (Admin) ───────── depends on 0.3 (needs API surface)
    ↓
Phase 0.5 (Web UI) ──────── depends on 0.4 (needs admin endpoints)
    ↓
Phase 0.6 (Filters) ─────── depends on 0.2 (needs engine hooks)
    ↓
Phase 0.7 (Integration) ─── depends on all above
    ↓
Phase 0.8 (Hardening) ───── depends on integration
    ↓
Phase 0.9 (Documentation) ─ depends on hardened code
    ↓
Phase 1.0 (Release) ─────── ship it
```

**Critical path:** 0.1 → 0.2 → 0.3 → 0.4 → 0.7 → 0.8 → 1.0

**Parallel work possible:**
- Phase 0.5 (Web UI) can start after 0.4, run parallel to 0.6
- Phase 0.6 (Filters) can start after 0.2, run parallel to 0.3–0.5
- Phase 0.9 (Docs) can start during 0.8

---

## Version Numbering

- **0.x.y** — Pre-release, breaking changes possible
- **x.0.0** — Stable release, semver applies
- **1.0.0** — First production-ready release

**Release cadence:** 0.x releases as features complete, 1.0.0 when all phases done.

---

*This plan is a living document. Update as implementation progresses.*
