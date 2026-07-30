# Stampd — Failure Modes & Recovery

**Source:** spec.md §9
**Last updated:** 2026-07-30

---

## Core Principle

Mail delivery is the core promise. Every other service can fail without
degrading inbound mail acceptance. This document covers every failure mode
across the system — what triggers it, how you'd know, what breaks, how to
fix it, and how to prevent it.

---

## 1. Service Failures

### 1a. Engine (`stampd-engine`) Crashes

- **Trigger:** OOM kill, uncaught panic, SQLite panic, TLS error, malformed
  SMTP input causing memory blowup, or OS kills the process.
- **Symptoms:** SMTP port 25/587 stops accepting connections. `stampd status`
  shows engine down. Inbound senders get connection refused. Queue processor
  stops.
- **Impact:** All inbound mail rejected (port 25). All outbound submission
  blocked (port 587). Delivery queue freezes. Existing Maildir files intact
  on disk.
- **Recovery:** `stampd-cli` backoff-restarts (1s → 2s → 4s → 8s → 30s max).
  After 10 consecutive failures, stops and logs — manual intervention needed.
  `systemctl restart stampd` or `stampd up`.
- **Prevention:** Set memory limits via systemd `MemoryMax`. Keep swap
  available. Run under `stampd-cli` supervisor (not bare). Monitor process
  uptime.

### 1b. Gateway (`stampd-gateway`) Crashes

- **Trigger:** Unhandled exception, Node OOM, invalid config, upstream
  dependency failure (database unreachable).
- **Symptoms:** HTTP port 8080 stops responding. Web UI shows connection
  error. Third-party integrations fail. `stampd status` shows gateway down.
- **Impact:** Zero impact on mail flow. Engine and admin unaffected (no
  dependency chain). No API or UI access until restored.
- **Recovery:** `stampd-cli` restarts automatically. `curl localhost:8080/health`
  to verify. If repeated crashes, check logs for root cause (usually
  database lock or memory).
- **Prevention:** Pin Node memory via `--max-old-space-size`. Keep database
  connection pool bounded. Monitor `GET /health` endpoint.

### 1c. Admin (`stampd-admin`) Crashes

- **Trigger:** Python exception, database connection pool exhaustion, OOM.
- **Symptoms:** Port 8081 stops responding. `GET /health` returns 500 or
  connection refused. `stampd status` shows admin down.
- **Impact:** Engine keeps accepting/queuing inbound mail. Outbound delivery
  continues from existing queue. Gateway returns `503` for signup/token
  operations (not hang). Admin UI broken. No user management.
- **Recovery:** `stampd-cli` restarts automatically. Check Python traceback
  in logs. Usually database lock contention — retry after 30s.
- **Prevention:** Configure SQLite WAL mode. Set connection pool timeout.
  Monitor admin `/health`.

### 1d. Web (`stampd-web`) Crashes

- **Trigger:** Build error, port conflict, upstream gateway unreachable.
- **Symptoms:** Port 3000 stops responding. Browser shows connection error.
- **Impact:** Zero impact on mail flow or API. Web UI is a plain API
  client — same as any third party.
- **Recovery:** `stampd-cli` restarts automatically. No urgency — mail
  continues flowing.
- **Prevention:** Not critical for uptime. Consider disabling in production
  if using a custom UI (`web.enabled = false`).

---

## 2. Database Failures

### 2a. SQLite Corruption

- **Trigger:** Power loss during write, disk sector failure, concurrent
  writes without WAL mode, OS crash.
- **Symptoms:** Application logs show `SQLITE_CORRUPT` or `SQLITE_NOTADB`.
  Health checks fail. Queries return errors.
- **Impact:** Authentication broken (can't look up users). Token validation
  fails. Queue processing stalls. Inbound SMTP still accepts and stores
  mail (Maildir is independent). Outbound submission blocked (needs auth).
- **Recovery:** `sqlite3 stampd.db ".recover" > recovered.sql` then
  `sqlite3 stampd-new.db < recovered.sql`. Replace corrupted DB with
  recovered version. If unrecoverable, start fresh (loses metadata, keeps
  Maildir files). Restore from backup if available.
- **Prevention:** Always use WAL mode (`PRAGMA journal_mode=WAL`). Run
  `PRAGMA integrity_check` weekly via cron. Keep database on reliable
  storage (not tmpfs). Backup daily.

### 2b. Disk Full (Database)

- **Trigger:** Database file grows to fill partition. Logs or temp files
  consuming space.
- **Symptoms:** `SQLITE_FULL` errors. Writes fail. New messages can't be
  queued. Sessions can't be stored.
- **Impact:** Same as corruption — auth, tokens, queue broken. Inbound
  SMTP still accepts (Maildir may also be full, see §8).
- **Recovery:** Delete dead-lettered messages (`DELETE FROM delivery_queue
  WHERE status='dead'`). Archive old delivery logs. Expand disk or move
  database to larger partition.
- **Prevention:** Monitor database size. Set up log rotation. Alert at 80%
  disk usage. Use separate partition for mail data.

### 2c. Lock Contention

- **Trigger:** High concurrency — many simultaneous SMTP connections
  attempting auth, queue processor competing with admin writes.
- **Symptoms:** Slow query times. `SQLITE_BUSY` errors in logs. Request
  timeouts in gateway/admin. Intermittent 503 responses.
- **Impact:** Degraded performance, not total failure. Some requests fail
  with timeout. Queue processing slows.
- **Recovery:** Wait for contention to clear. Restart services if lock
  held indefinitely. Check for long-running transactions.
- **Prevention:** WAL mode reduces contention. Keep transactions short.
  Use `PRAGMA busy_timeout=5000`. Avoid holding connections open across
  await points.

---

## 3. Network Failures

### 3a. DNS Resolution Failure

- **Trigger:** System DNS resolver unreachable, `/etc/resolv.conf`
  misconfigured, DNS server down.
- **Symptoms:** MX lookup fails for outbound delivery. SPF check returns
  soft-pass (best-effort fallback). Logs show `trust_dns_resolver` errors.
- **Impact:** Outbound mail can't be delivered (MX lookup is first step).
  Inbound SPF checks skipped (accepts mail anyway). Inbound mail unaffected.
- **Recovery:** Fix DNS configuration. `resolvectl status` to check.
  Restart systemd-resolved if needed. Outbound queue retries automatically.
- **Prevention:** Use multiple DNS resolvers (8.8.8.8, 1.1.1.1 as
  fallback). Monitor DNS resolution from the host. Keep local resolver
  healthy.

### 3b. MX Lookup Timeout

- **Trigger:** Remote domain's DNS slow to respond, network congestion,
  UDP packet loss.
- **Symptoms:** `MX lookup failed: timeout` in logs. Delivery marked as
  temporary failure. Queue retries with backoff.
- **Impact:** Delayed delivery, not permanent loss. Message stays in queue
  with `next_attempt_at` set.
- **Recovery:** Automatic — queue retries with exponential backoff. If
  DNS is chronically slow, check network path and MTU.
- **Prevention:** trust-dns-resolver has built-in timeouts. Monitor
  outbound delivery latency. Consider caching MX records locally.

### 3c. Connection Refused (Outbound)

- **Trigger:** Remote MX server port 25 down, firewall blocking
  outbound SMTP, IP on blocklist.
- **Symptoms:** `Connection to {mx} failed: connection refused` in logs.
  Delivery marked temporary failure. Queue retries.
- **Impact:** Same as timeout — delayed delivery, automatic retry.
  If IP is blocklisted, all mail to that provider bounces.
- **Recovery:** If blocklisted, request delisting from provider. If
  firewall, allow outbound port 25. Queue handles retries.
- **Prevention:** Monitor outbound delivery rates. Maintain clean IP
  reputation. Check blocklist status periodically (MXToolbox, etc.).

### 3d. TLS Handshake Failure (Outbound)

- **Trigger:** Remote server requires TLS but certificate verification
  fails, protocol mismatch, or STARTTLS downgrade attack.
- **Symptoms:** `STARTTLS` negotiation fails. Engine continues in
  plaintext (best-effort per implementation). Logs show TLS error.
- **Impact:** Mail sent in plaintext if remote server accepts. If remote
  requires TLS, delivery fails with temporary error.
- **Recovery:** Automatic — queue retries. If persistent, check remote
  server's TLS configuration. Consider disabling TLS requirement for
  that domain if acceptable.
- **Prevention:** Keep rustls/ring dependencies updated. Test TLS
  configuration against remote servers. Log TLS status per delivery.

---

## 4. Authentication Failures

### 4a. Invalid Credentials

- **Trigger:** User enters wrong password, expired API token, revoked
  token sent in Bearer header.
- **Symptoms:** HTTP 401 response. Login page shows "invalid credentials".
  API client gets 401 with error body.
- **Impact:** Only affects the authenticated user. Other users unaffected.
  No mail delivery impact.
- **Recovery:** User retries with correct credentials. Admin can reset
  password or issue new token.
- **Prevention:** Show clear error messages ("invalid password" vs
  "account disabled"). Rate-limit login attempts (§4d).

### 4b. Expired Sessions

- **Trigger:** Session cookie older than `expires_at` in `sessions` table.
  Browser cleared cookies.
- **Symptoms:** User redirected to login page. API requests without valid
  session return 401.
- **Impact:** Single user loses web UI access. No mail flow impact.
- **Recovery:** User logs in again. Session is created fresh.
- **Prevention:** Set reasonable session TTL (24h default). Implement
  "remember me" for longer sessions if desired.

### 4c. Disabled Accounts

- **Trigger:** Admin sets `disabled_at` timestamp on user. User account
  deleted.
- **Symptoms:** Login returns 403 "account disabled". SMTP AUTH rejects
  credentials. API token validation fails.
- **Impact:** Affected user cannot send or access web UI. Their mailbox
  remains on disk. Other users unaffected.
- **Recovery:** Admin re-enables account (clears `disabled_at`) or
  creates new account.
- **Prevention:** Notify user before disabling. Keep mailbox data for
  configurable retention period after disable.

### 4d. Brute Force Protection

- **Trigger:** >10 failed login attempts from same IP within 5 minutes.
  Repeated token auth failures.
- **Symptoms:** HTTP 429 "too many requests" after threshold. IP
  temporarily blocked at gateway rate limiter.
- **Impact:** Legitimate users from same IP may be temporarily blocked.
  Account lockout after N failures (configurable).
- **Recovery:** Wait for rate limit window to expire (1 minute per
  `rate_limit_per_min`). Admin can manually unblock. Reset failed
  attempt counter on successful login.
- **Prevention:** Use strong passwords. Implement account lockout after
  10 failures. Log all failed attempts for audit. Consider fail2ban
  integration at OS level.

---

## 5. Mail Delivery Failures

### 5a. Bounce Handling

- **Trigger:** Remote server returns 5xx (permanent failure) or 4xx
  (temporary failure) after DATA.
- **Symptoms:** Delivery marked as `bounced` or `temp_failed` in
  `delivery_queue`. `last_error` contains SMTP response.
- **Impact:** 5xx → message dead-lettered immediately. 4xx → retry
  with backoff, dead-letter after `MAX_ATTEMPTS` (5).
- **Recovery:** 5xx bounces: notify sender (future feature) or admin
  reviews dead-letter queue. 4xx: automatic retry. Admin can manually
  retry via `POST /admin/queue/:id/retry`.
- **Prevention:** Monitor bounce rates. High bounce rate to specific
  domain may indicate DNS or reputation issue.

### 5b. Dead Letter Queue

- **Trigger:** Delivery failed after `MAX_ATTEMPTS` (5) retries.
  Message file missing from disk.
- **Symptoms:** Message status set to `dead` in `delivery_queue`.
  Admin queue view shows dead-lettered messages.
- **Impact:** Message never delivered. Sender not notified (v1).
  Storage consumed by dead `.eml` files.
- **Recovery:** Admin reviews via `GET /admin/queue` (filter by
  `status=dead`). Manual retry if transient issue resolved. Delete
  to free space (`DELETE /admin/queue/:id`).
- **Prevention:** Monitor queue depth. Alert on high dead-letter rate.
  Implement bounce notification in v2+.

### 5c. Retry Exhaustion

- **Trigger:** Remote server consistently returns 4xx (e.g., mailbox
  full, server overloaded) for 5 attempts.
- **Symptoms:** Message moves from `pending` to `dead` status.
  `attempts` field reaches `MAX_ATTEMPTS`.
- **Impact:** Same as dead letter — message undelivered.
- **Recovery:** Admin investigates `last_error`. If issue is transient
  (remote server was temporarily full), manual retry may succeed.
- **Prevention:** Exponential backoff already implemented (5s intervals
  in queue processor). Monitor `last_error` patterns.

### 5d. Message File Missing

- **Trigger:** `.eml` file deleted from Maildir (manual cleanup, disk
  error, accidental `rm`). Queue references non-existent path.
- **Symptoms:** Queue processor logs `Message file missing, marking as
  failed`. Message marked `dead` immediately.
- **Impact:** Outbound message lost. Inbound unaffected (mail already
  delivered to Maildir before queuing).
- **Recovery:** Message is unrecoverable from queue. If inbound, check
  Maildir directly. If outbound, sender must re-send.
- **Prevention:** Don't manually delete files from Maildir queue area.
  Use admin API for queue management. Monitor for orphaned queue entries.

---

## 6. Filter Failures

### 6a. Filter Script Crashes

- **Trigger:** Bug in user-defined Python/JS filter script. Missing
  dependency. Syntax error.
- **Symptoms:** Script exits with non-zero status. Engine logs
  `Filter execution failed`. Mail delivery continues.
- **Impact:** Filter is skipped for that hook point. Mail delivered
  without filtering. Other filters still run.
- **Recovery:** Fix script bug. Check stderr output in engine logs.
  Test filter standalone with JSON input on stdin.
- **Prevention:** Test filters before deploying. Use the filter SDK
  for consistent error handling. Log filter errors to stderr.

### 6b. Filter Timeout

- **Trigger:** Filter script hangs (infinite loop, blocked I/O,
  network call to dead service). Execution exceeds `filters.timeout_ms`
  (default 500ms).
- **Symptoms:** Engine logs `Filter timed out after {ms}ms`. Process
  killed (`kill_on_drop(true)`). Mail continues.
- **Impact:** Filter skipped. Mail delivered unfiltered. No other
  impact — this is the designed degradation path.
- **Recovery:** Fix filter to complete within timeout. Increase
  `filters.timeout_ms` if legitimate filter needs more time. Profile
  filter performance.
- **Prevention:** Keep filter logic fast (<100ms typical). Avoid
  network calls in filters. Use `filters.timeout_ms` as safety net.

### 6c. Invalid Filter Output Format

- **Trigger:** Filter script outputs non-JSON, missing `action` field,
  or `action` is not "accept"/"reject".
- **Symptoms:** Engine logs `Failed to parse filter output`. Mail
  delivery continues (error treated as accept).
- **Impact:** Filter effectively skipped. Mail delivered unfiltered.
- **Recovery:** Fix filter output format. Must return JSON:
  `{"action": "accept", "reason": "..."}` or
  `{"action": "reject", "reason": "spam detected"}`.
- **Prevention:** Use filter SDK which handles output formatting.
  Validate output format in filter tests.

---

## 7. DKIM/SPF/DMARC Failures

### 7a. DKIM Key Missing

- **Trigger:** `.pkcs8` file not found in `dkim_key_dir`. First run
  before key generation, or key deleted.
- **Symptoms:** Engine logs `Failed to initialize DKIM signer —
  outgoing mail unsigned`. Startup continues but DKIM is disabled.
- **Impact:** All outbound mail sent without DKIM signature. Recipient
  servers may reject or spam-filter messages. SPF/DMARC may fail
  alignment.
- **Recovery:** Generate key pair:
  ```sh
  openssl genrsa -out {dkim_key_dir}/{selector}.pem 2048
  openssl pkcs8 -topk8 -inform PEM -outform DER \
    -in {dkim_key_dir}/{selector}.pem \
    -out {dkim_key_dir}/{selector}.pkcs8 -nocrypt
  ```
  Restart engine. Publish public key to DNS (saved to `{selector}.dns.txt`).
- **Prevention:** Generate keys before first production send. Include
  key generation in deployment script. Verify DKIM-Signature header
  on outbound test mail.

### 7b. DKIM Signing Error

- **Trigger:** Key file corrupted, wrong format (not PKCS8), key too
  small (<2048 bit), `ring` crypto library error.
- **Symptoms:** Engine logs `DKIM signing failed`. Outbound mail sent
  without signature.
- **Impact:** Same as key missing — unsigned mail.
- **Recovery:** Regenerate key pair. Verify PKCS8 DER format. Check
  key size (`openssl rsa -in key.pem -text -noout`).
- **Prevention:** Use provided key generation commands. Don't manually
  convert key formats. Test signing before production.

### 7c. SPF DNS Lookup Failure

- **Trigger:** DNS resolver unreachable, remote domain has no SPF
  record, TXT record lookup fails.
- **Symptoms:** Engine logs `DNS TXT lookup failed for SPF` or
  `No SPF record found`. Returns best-effort pass.
- **Impact:** No impact on inbound mail acceptance (best-effort per
  spec). SPF result logged but not enforced.
- **Recovery:** None needed — this is by design. If you want SPF
  enforcement, modify `spf.rs` to reject on hard fail.
- **Prevention:** Ensure DNS resolver is healthy. Accept that SPF is
  informational in v1.

### 7d. DMARC Not Implemented (v1)

- **Trigger:** By design — DMARC enforcement is out of scope for v1
  (spec §15).
- **Symptoms:** No DMARC alignment check on inbound. No DMARC record
  published for your domain (optional in v1).
- **Impact:** Recipient servers may check DMARC alignment on your
  outbound mail. If DKIM + SPF both fail alignment, some providers
  may reject.
- **Recovery:** Publish DMARC record for your domain (even without
  enforcement, `p=none` gives you reports). Ensure DKIM and SPF
  are correct.
- **Prevention:** Set up DKIM and SPF correctly. Publish DMARC `p=none`
  for monitoring. Plan DMARC enforcement for v2+.

---

## 8. Disk/Storage Failures

### 8a. Maildir Full

- **Trigger:** Partition hosting `/var/lib/stampd/mail` runs out of
  space. Quota limit reached.
- **Symptoms:** Engine logs disk write errors during DATA phase.
  SMTP returns `452 Insufficient storage`. Outbound queue can't
  write new `.eml` files.
- **Impact:** New inbound mail rejected at DATA stage. New outbound
  messages can't be enqueued. Existing queued messages may fail to
  deliver if temp files needed.
- **Recovery:** Delete old/dead-lettered messages from Maildir.
  Archive to cold storage. Expand disk. Increase user quotas via
  admin (`default_quota_mb`).
- **Prevention:** Monitor disk usage. Alert at 80%. Set up log
  rotation. Implement automated cleanup of dead-letter queue.

### 8b. Outbox/Queue Full

- **Trigger:** Delivery queue grows unbounded due to persistent
  delivery failures to remote servers.
- **Symptoms:** `delivery_queue` table grows. Database file grows.
  Queue processor cycles through same messages repeatedly.
- **Impact:** Slow queue processing (scanning many dead entries).
  Database performance degrades. Disk fills up.
- **Recovery:** Bulk-delete dead entries: `DELETE FROM delivery_queue
  WHERE status='dead' AND attempts >= 5`. Archive to cold storage
  if needed.
- **Prevention:** Monitor queue depth. Alert on >1000 pending.
  Auto-prune dead entries older than 30 days (cron job).

### 8c. Temp File Cleanup

- **Trigger:** Engine process killed mid-write. Filter scripts
  writing to temp directories. Crash during delivery.
- **Symptoms:** Orphaned `.tmp` files in Maildir. Temp files
  in `/tmp` from filter execution.
- **Impact:** Disk space consumed. No functional impact unless
  disk fills up.
- **Recovery:** Manual cleanup: `find /var/lib/stampd/mail -name "*.tmp" -delete`.
  Restart engine (cleans up its own temp files on startup).
- **Prevention:** Implement temp file cleanup on startup. Use
  `kill_on_drop(true)` for child processes (already done for
  filters). Periodic cron cleanup.

---

## 9. Configuration Failures

### 9a. Invalid TOML Syntax

- **Trigger:** Malformed `stampd.toml` — missing quotes, unclosed
  brackets, invalid characters.
- **Symptoms:** Engine fails to start. Error message points to
  parse location. `stampd up` shows config error.
- **Impact:** Service won't start. No mail flow.
- **Recovery:** Fix TOML syntax. Validate with `toml-cli` or
  `python -c "import tomllib; tomllib.load(open('stampd.toml'))"`.
  Restart.
- **Prevention:** Validate config in CI. Use `stampd.toml` example
  as template. Run config validation on startup before binding ports.

### 9b. Missing Required Fields

- **Trigger:** Required config fields not set: `engine.domain`,
  `engine.maildir_path`, `engine.db_path`.
- **Symptoms:** Engine panics on unwrap/expect. Error message
  indicates missing field. Service won't start.
- **Impact:** Service won't start.
- **Recovery:** Add missing fields to `stampd.toml`. Use defaults
  from spec §7. Restart.
- **Prevention:** Validate all required fields on startup. Print
  clear error with field name and expected type. Include example
  config in docs.

### 9c. Config Reload Failure (SIGHUP)

- **Trigger:** Send SIGHUP to engine process with invalid config
  in `stampd.toml`.
- **Symptoms:** Engine logs `Failed to reload config via SIGHUP`.
  Previous config remains active. No restart needed.
- **Impact:** Config change not applied. Old config continues
  working. No service interruption.
- **Recovery:** Fix config file. Send SIGHUP again. Or restart
  service for clean load.
- **Prevention:** Validate config before sending SIGHUP. Test
  with `stampd up` first. Keep config changes atomic (write
  complete file, not partial edits).

### 9d. Port Conflict

- **Trigger:** Another process already bound to configured port
  (25, 587, 8080, 8081, 3000).
- **Symptoms:** Engine/gateway/admin fails to bind. Error:
  `Address already in use`. Service won't start.
- **Impact:** Service won't start. Other services on different
  ports unaffected.
- **Recovery:** Identify process using port (`ss -tlnp | grep :25`).
  Kill conflicting process or change port in config. Restart.
- **Prevention:** Check ports before starting (`stampd status`).
  Use systemd socket activation for production. Document port
  requirements.

---

## 10. Security Incidents

### 10a. Credential Compromise

- **Trigger:** API token leaked in logs, git history, or shared
  channel. Password cracked (weak password).
- **Symptoms:** Unusual send activity from compromised account.
  Delivery logs show unknown recipients. User reports unauthorized
  access.
- **Impact:** Attacker can send mail as compromised user. May
  abuse server reputation. May access mailbox contents via API.
- **Recovery:** Immediately revoke compromised token (`DELETE
  /admin/tokens/:id`). Reset user password. Review delivery logs
  for scope of abuse. Check blocklist status.
- **Prevention:** Never log tokens. Hash tokens at rest (done in
  schema). Enforce strong passwords. Implement token expiry (future).
  Rate-limit send operations per user.

### 10b. Unauthorized Access

- **Trigger:** Session hijacking, CSRF attack, SQL injection in
  gateway/admin, auth bypass bug.
- **Symptoms:** Unexpected API calls. Admin operations performed
  by non-admin user. New users created without signup.
- **Impact:** Depends on scope — could range from mailbox read
  to full admin takeover.
- **Recovery:** Revoke all sessions (`DELETE FROM sessions`).
  Revoke all tokens. Force password reset for all users. Audit
  logs for attack scope. Patch vulnerability.
- **Prevention:** Use parameterized queries (SQLx/rusqlite do
  this). Validate all input. CSRF tokens on state-changing
  requests. Rate-limit all endpoints. Regular security audit
  (phase 0.8.0).

### 10c. Abuse Detection

- **Trigger:** Server used as open relay (shouldn't happen — RCPT
  TO validation prevents this). Spam sent through compromised
  credentials. High volume to single recipient.
- **Symptoms:** Delivery logs show high send rate. Remote servers
  report spam complaints. IP appears on blocklists.
- **Impact:** Server IP reputation damaged. All outbound mail may
  be rejected by major providers. Domain may be blacklisted.
- **Recovery:** Identify compromised account. Revoke credentials.
  Request delisting from blocklists. Reduce send volume temporarily.
  Notify affected recipients if possible.
- **Prevention:** Enforce `RCPT TO` domain validation (done).
  Require authentication for all outbound (done). Rate-limit per
  user. Monitor outbound volume. Implement abuse reporting endpoint.
  Maintain rDNS/PTR records.

---

## Recovery Procedures Reference

### Service Crash Recovery

1. `stampd-cli` detects process exit via `waitpid`
2. Backoff restart: 1s → 2s → 4s → 8s → 30s (max)
3. After 10 consecutive failures, stop restarting
4. Log restart attempt with service name and exit code
5. Manual: `systemctl restart stampd` or `stampd up`

### Database Recovery

1. Check integrity: `sqlite3 stampd.db "PRAGMA integrity_check;"`
2. If corrupted: `.recover` mode to extract salvageable data
3. If no backup: start fresh (maildir files preserved on disk)
4. If backup exists: `cp stampd.db.bak stampd.db` and restart
5. Prevent: WAL mode + daily backup + integrity check cron

### Disk Full Recovery

1. `df -h /var/lib/stampd` — identify partition
2. Delete dead-letter queue: `DELETE FROM delivery_queue WHERE status='dead';`
3. Remove orphaned Maildir files: `find /var/lib/stampd/mail -name "*,*" -mtime +30 -delete`
4. Archive old delivery logs
5. Expand partition or add storage

---

## Monitoring Quick Reference

| Metric | Source | Threshold | Action |
|--------|--------|-----------|--------|
| SMTP connections | engine | > 100 concurrent | Check for abuse |
| Queue depth | engine | > 1000 pending | Investigate delivery failures |
| Filter timeout rate | engine | > 5% of calls | Fix slow filters |
| Gateway p99 latency | gateway | > 500ms | Check database contention |
| Database size | admin | > 1GB | Archive old data |
| Maildir usage | admin | > 80% quota | Expand or clean up |
| Dead-letter rate | engine | > 10% of sends | Check recipient domains |
| Failed auth rate | gateway | > 20/min per IP | Possible brute force |

---

*This document must be updated before any production deployment. Review
after each release and after any incident.*
