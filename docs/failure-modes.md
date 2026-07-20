# Stampd — Failure Mode Contract (v1)

**Source:** spec.md §9
**Last updated:** 2026-07-20

---

## Core Principle

Mail delivery is the core promise. Every other service can fail without
degrading mail flow. This document defines the required behavior for each
failure scenario.

---

## Failure Scenarios

### 1. `admin` Down, `engine` Still Receiving Mail

**Condition:** The admin service (`stampd-admin`) crashes or is unreachable.

**Required behavior:**
- Engine keeps accepting/queuing inbound mail — mail delivery must not
  degrade due to admin-plane issues
- Gateway must reject signup/token-issuance calls with a clear `503 Service
  Unavailable`, not hang or timeout
- Engine continues outbound delivery from existing queue

**What still works:**
- Inbound SMTP (port 25)
- Outbound submission (port 587) for authenticated senders
- Delivery queue processing
- Existing mailbox access via gateway

**What breaks:**
- New user signup
- Token creation/revocation
- Admin operations (user management, config changes)
- Delivery log writes (queued for later)

**Detection:** Gateway health endpoint returns degraded status.

---

### 2. `gateway` Down

**Condition:** The gateway service (`stampd-gateway`) crashes or is unreachable.

**Required behavior:**
- Engine and admin are unaffected (no dependency on gateway)
- No UI/API access until gateway is restored
- All mail flow continues uninterrupted

**What still works:**
- Inbound SMTP (port 25)
- Outbound submission (port 587)
- Delivery queue processing
- Admin operations (internal API)

**What breaks:**
- Web UI (`stampd-web`) — cannot connect
- Third-party UIs — cannot connect
- All REST API access

**Detection:** Web UI shows connection error. Admin service logs gateway down.

---

### 3. `web` Down

**Condition:** The web UI service (`stampd-web`) crashes or is unreachable.

**Required behavior:**
- Zero impact on mail flow or API — it's a client like any third party
- Gateway continues serving API requests
- All mail operations continue

**What still works:**
- Everything except the web UI
- All API endpoints (gateway)
- All mail flow (engine)
- All admin operations (admin)

**What breaks:**
- Web UI access only

**Detection:** N/A — this is expected to be a client-side failure.

---

### 4. `engine` Down

**Condition:** The engine service (`stampd-engine`) crashes or is unreachable.

**Required behavior:**
- Inbound mail: SMTP connections refused (port 25 not listening)
- Outbound mail: Cannot send new messages
- Gateway returns appropriate errors for mailbox/send operations
- Admin can still manage users/tokens (metadata operations)

**What still works:**
- Web UI loads (but cannot fetch messages or send)
- Gateway serves static data (user list, config)
- Admin operations (user/token management)
- Maildir files on disk are intact

**What breaks:**
- All inbound mail (SMTP port 25)
- All outbound mail (submission port 587)
- Mailbox read operations (gateway queries engine)
- Queue processing

**Detection:** Gateway health endpoint returns degraded. CLI status shows engine down.

---

### 5. `filters` Down / Hung

**Condition:** Filter service crashes or a filter hook hangs.

**Required behavior:**
- Engine continues mail delivery — filter timeouts prevent stalls
- `filters.timeout_ms` (default 500ms) bounds hook execution time
- Hung filter is killed after timeout, mail continues

**What still works:**
- All mail flow (inbound and outbound)
- Queue processing
- All other services

**What breaks:**
- Filter hooks are skipped (mail delivered without filtering)
- Filter logs may be incomplete

**Detection:** Engine logs filter timeout/error. Delivery continues.

---

### 6. SQLite Database Corrupted / Unavailable

**Condition:** SQLite database file is corrupted or disk is full.

**Required behavior:**
- Engine continues inbound mail acceptance (Maildir writes are independent)
- Outbound queue may stall (needs database for retry tracking)
- Gateway returns 503 for operations requiring database
- Admin cannot perform database operations

**What still works:**
- Inbound SMTP (port 25)
- Maildir file reads (existing mail)
- Outbound submission (port 587) for direct sends

**What breaks:**
- User authentication (password lookup)
- Token validation
- Queue processing (retry tracking)
- Admin operations (user/config management)

**Detection:** Application logs show SQLite errors. Health checks fail.

---

### 7. Maildir Disk Full

**Condition:** Disk containing Maildir runs out of space.

**Required behavior:**
- Inbound SMTP: Accept message but fail to write, return 452 "Insufficient storage"
- Outbound: Queue messages but cannot write .eml files
- Gateway returns quota exceeded for send operations

**What still works:**
- SMTP connections accepted (until DATA phase)
- Queue processing (existing queued messages)
- Admin operations (metadata only)

**What breaks:**
- New inbound mail storage
- New outbound message creation
- Mailbox read for messages not yet flushed

**Detection:** Engine logs disk write errors. Admin shows quota warnings.

---

## Recovery Procedures

### Service Crash
1. `stampd-cli` detects process exit
2. Backoff restart policy: 1s → 2s → 4s → 8s → 30s max
3. After 10 consecutive failures, stop restarting (manual intervention)
4. Log restart attempt with service name and error

### Database Recovery
1. Backup SQLite database regularly (cron job recommended)
2. If corrupted, restore from backup
3. If no backup, engine can start with fresh database (loses metadata, keeps Maildir)

### Disk Full Recovery
1. Remove dead-lettered messages from queue
2. Archive old messages to cold storage
3. Increase disk quota or add storage

---

## Monitoring Points

| Metric | Source | Threshold |
|--------|--------|-----------|
| SMTP connection count | engine | Alert if > 100 concurrent |
| Queue depth | engine | Alert if > 1000 pending |
| Filter timeout rate | engine | Alert if > 5% of calls |
| Gateway response time | gateway | Alert if p99 > 500ms |
| Database size | admin | Alert if > 1GB |
| Maildir usage | admin | Alert if > 80% quota |

---

*This document must be updated before any production deployment.*
