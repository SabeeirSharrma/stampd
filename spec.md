# Stampd — Project Spec

**Status:** Spec-locked, pre-implementation
**Standalone project** — not part of The Cinder Project, lives in SabeeirSharrma - docs in sabeeir.qd.je/stampd

---

## 1. What Stampd Is

A self-hosted, single-domain mail server: receives inbound mail from anyone
(Gmail, Outlook, any sender) addressed to one configured domain, and sends
outbound mail only as that domain's identity. Multi-user, with individual
mailboxes, self-signup (admin-revocable), and both a reference web UI and a
public API for third-party UI/tooling.

**Not** a multi-tenant service in v1. **Not** an open relay — inbound only
accepts `RCPT TO` for the server's own domain; outbound only accepts
authenticated senders.

---

## 2. Naming

**Stampd** — locked. (Rejected: HiMail, vvSMTP, openSMTP, selfmail, vSMTP
[collides with existing Rust MTA by Viridit], LiteSMTP/FeatherSMTP/SlimSMTP
[SlimSMTP collides with Torxed/slimSMTP], Smtpka [too similar to Gitka —
portfolio pattern-repetition risk], smt-pi [risks reading as Raspberry
Pi-specific], supaSMTP [reads as "SMTP for Supabase," collides with Supabase's
own SMTP docs mindshare], Smtpr [rejected in favor of stronger pun].)

No collisions found for "Stampd" as of this spec's writing.

---

## 3. Roadmap

| Version | Scope |
|---|---|
| **v1** | SMTP core (send + receive), API-first architecture, customizable web UI + public API + docs, multilang service split, admin controls |
| **v2** | Proton-style zero-access encryption at rest |
| **v3** | Expanded admin controls (rate limits, spam/reputation tooling, audit log, multi-admin roles) |
| **v4** | Hosted multi-tenant service (GitLab model) — self-host remains fully supported; hosted offering gives 5GB free if not self-hosting |

This spec covers **v1** in full. v2–v4 are directional, not detailed, until
their turn comes.

---

## 4. Architecture — Modular Monolith

Four logically independent services, **one deployable unit**, each
independently configurable via a single `stampd.toml` (per-service section,
per-service enable/disable). Not independently deployed in v1 — no service
discovery, no separate scaling, no network hops between services in
production (Transit calls are in-process/local-socket).

```
stampd/
│
├── stampd-engine/        (Rust)   — safety-critical core, the trust boundary
│   ├── smtpd                inbound daemon, port 25, parses untrusted bytes
│   ├── submissiond            outbound daemon, port 587, AUTH + DKIM signing
│   ├── maildir                 mailbox storage (Maildir format)
│   └── queue                   outbound delivery queue, retry/backoff
│
├── stampd-gateway/       (Node/TS, Fastify) — public API surface
│   Auth validation (session + token), rate limiting, request validation,
│   OpenAPI spec source of truth. Every UI (stampd-web and third-party)
│   talks to this and only this. Never touches Maildir/SQLite directly.
│
├── stampd-admin/         (Python, FastAPI) — business/admin logic
│   User & org management, token issuance/revocation, domain config,
│   signup toggle, delivery logs, quota tracking.
│
├── stampd-filters/        hooks — user-defined scripts (Python/JS),
│                            invoked by stampd-engine at MAIL FROM / RCPT TO
│                            / DATA stages, via Transit
│
├── stampd-web/           (TS, Astro/React) — reference web UI
│                            A plain API consumer, no special privileges —
│                            proves the API is good enough for third parties.
│
└── stampd-cli/            (Rust) — supervisor + entrypoint
                             `stampd up`, `stampd status`, `stampd up --only ...`
```

### Why each language owns what it owns

- **Rust (`stampd-engine`)** — the only place parsing untrusted internet
  bytes, doing crypto (DKIM sign/verify via `rustls`/`ring`, never hand-rolled),
  and writing mail to disk without corruption. This is Rust's actual
  justification here — not used elsewhere for consistency's sake.
- **Node (`stampd-gateway`)** — I/O-bound, high-concurrency request routing,
  auth, rate limiting. The one contract every UI depends on, so it stays
  boring, fast, and stable.
- **Python (`stampd-admin`)** — CRUD-heavy business logic, fastest to iterate
  on for v3's expanded admin controls, good fit for future reporting/analytics.
- **Transit** — connects `engine ↔ gateway ↔ admin ↔ filters`. Internal-only.
  The public boundary (`gateway → external UIs`) stays plain REST/OpenAPI, so
  Transit's blast radius never reaches a customer-facing contract. This
  contains risk from Transit still being Phase 1 / unproven.

---

## 5. Transit Integration

Transit (The Cinder Project's cross-language interop framework) is the
connective tissue **inside** `stampd-engine`'s process boundary — it never
crosses the public API surface.

**Where Transit is used:**

- `stampd-engine ↔ stampd-filters` — the primary use case. User-defined
  filter/rule hooks (Python or JS) get called directly by the Rust engine at
  MAIL FROM / RCPT TO / DATA stages, using Transit's `// transit:function`
  marker export system instead of a REST/webhook round-trip. This is the
  case Transit is actually good for: low-latency, in-process/local-socket
  calls between trusted, co-located services.
- `stampd-engine ↔ stampd-gateway`, `stampd-engine ↔ stampd-admin` —
  internal calls (e.g. gateway asking engine for queue status, admin
  pushing a domain config change to engine) also go through Transit rather
  than each service standing up its own internal HTTP client/server pair.

**Where Transit is explicitly *not* used:**

- `stampd-gateway → external UIs` (stampd-web, third-party UIs) — this
  boundary stays plain REST/OpenAPI, always. This is a deliberate
  containment strategy: Transit is still Phase 1 and unproven at
  production scale. Keeping it internal-only means if Transit has a rough
  edge, the blast radius is confined to services you control and can patch
  together — never a customer-facing contract that a third-party UI
  depends on.

**Risk this doesn't remove:** `stampd-engine` (the safety-critical,
internet-facing core) now has a real dependency on Transit's correctness
for filter-hook execution and internal service comms. A bug in Transit
could still affect mail delivery indirectly (e.g. a filter hook call that
hangs or misbehaves) even though the public API is insulated. The
`filters.timeout_ms` config (Section 6) exists specifically to bound this
risk — a hung filter hook must not be able to stall the engine.

**Decision carried over from earlier discussion, still open:** whether
Stampd is the project that proves Transit out under real (if internal-only)
production conditions, or whether Transit should get proven on lower-stakes
ground first and get wired into Stampd once more mature. Current spec
proceeds on the assumption Stampd does the proving, contained to the
filters/internal-comms boundary described above — revisit if that
assumption stops feeling right once implementation starts.

---

## 5a. Web UI Customization — Bring Your Own Frontend

Because `stampd-gateway` is the *only* integration point (Section 5 above,
API-First Principle), any org or user can build and run their own UI
against a Stampd instance without touching Rust, Python, or Node code at
all. `stampd-web` is not special — it's a reference implementation proving
the API is sufficient on its own.

**What Stampd ships to support this:**

- **OpenAPI spec** — the source of truth, published alongside the gateway.
  Every endpoint (auth, mailbox read, compose/send, token management,
  admin operations) is documented here first; docs are generated from it,
  never hand-maintained separately.
- **Auth for third-party UIs** — same mechanism as everything else: a
  logged-in session (cookie) for browser-based custom UIs, or an API token
  for headless/automated UIs. No separate "partner" auth tier — deliberately
  the same path internal `stampd-web` uses, so nothing is held back from
  external builders.
- **Reference client libraries** (stretch goal for v1, not a hard
  requirement) — thin TS and Python wrappers around the OpenAPI spec, so
  someone building a custom UI isn't starting from raw `fetch` calls.
- **CORS config** (`gateway.cors_origins` in `stampd.toml`) — operators
  running their own UI on a separate origin need this open to their domain;
  documented as part of the "build your own UI" quickstart.

**What "customization" means concretely, in priority order:**

1. **Full replacement** — an org disables `stampd-web` entirely
   (`web.enabled = false`) and points their own frontend at
   `stampd-gateway`. Fully supported, this is the primary design target.
2. **Theming the reference UI** — for orgs that want the shipped
   `stampd-web` but with their own branding (logo, colors, name). Lower
   priority than (1) for v1; a config-driven theme layer in `stampd-web`
   itself (not a new API surface) is enough — worth scoping only after (1)
   is solid.

**Not in v1 scope:** a plugin/extension system *within* `stampd-web` (e.g.
custom widgets, embedded third-party panels). That's a meaningfully
different feature (extensibility of one app) from what's being asked for
here (freedom to replace the app entirely) — if it's wanted later, treat it
as its own spec, not an extension of this one.

---

## 6. API-First Principle

`stampd-gateway` exposes the **only** integration point into the system.
`stampd-web` is not privileged — it is "API client #1," consuming the exact
same OpenAPI-documented endpoints any third-party org would use to build
their own UI. Docs are generated from the OpenAPI spec, not hand-maintained
separately, so they can't drift.

Practical implication: **write the OpenAPI spec before writing `stampd-web`.**

---

## 7. Config — `stampd.toml`

```toml
[engine]
smtp_port = 25
submission_port = 587
maildir_path = "/var/lib/stampd/mail"
dkim_selector = "default"

[gateway]
enabled = true
port = 8080
rate_limit_per_min = 60
cors_origins = ["*"]

[admin]
enabled = true
port = 8081            # internal only, not publicly exposed
signup_enabled = true
default_quota_mb = 5120

[web]
enabled = true         # false if only running stampd-web separately,
                        # or if the operator brings their own UI
port = 3000

[filters]
enabled = true
timeout_ms = 500        # a hanging filter hook must not stall mail delivery
```

---

## 8. Process Supervision (`stampd-cli`)

A Rust supervisor, not a thin wrapper:

- Reads `stampd.toml` once, spawns only the services marked `enabled = true`
- Owns restart policy per child process (crash → backoff-restart)
- Tags/streams each child's logs by service name for legible aggregate
  logging (`journalctl -u stampd` stays readable across 4 processes)

```
stampd up                          # start everything enabled in config
stampd up --only engine,gateway    # dev mode, skip admin/web
stampd status                       # show running services, ports, health
```

**Deployment target:** one `stampd.service` systemd unit running the
supervisor — matches the GitFlare deploy pattern (systemd + Caddy) rather
than multiplying units to maintain.

---

## 9. Failure-Mode Contract

Must be true regardless of which service is down:

| Condition | Required behavior |
|---|---|
| `admin` down, `engine` still receiving mail | Engine keeps accepting/queuing inbound mail — mail delivery is the core promise and must not degrade due to admin-plane issues. `gateway` must reject signup/token-issuance calls with a clear 503, not hang. |
| `gateway` down | `engine`/`admin` unaffected (no dependency on gateway). No UI/API access until restored. |
| `web` down | Zero impact on mail flow or API — it's a client like any third party. |

To be written as `docs/failure-modes.md` in the repo before scaffolding —
cheap to write now, expensive to reconstruct mid-incident later.

---

## 10. Data Model (v1, single-domain)

Mail content lives in **Maildir** (inbound) or as raw `.eml` files referenced
by path (outbound queue) — never in SQLite. SQLite holds only metadata,
auth, and routing.

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,          -- user@yourdomain.com
    password_hash TEXT NOT NULL,          -- argon2id; AUTH PLAIN + web UI login
    is_admin BOOLEAN NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    disabled_at INTEGER                   -- null = active
);

CREATE TABLE auth_tokens (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    token_hash TEXT NOT NULL,             -- hash only; raw token shown once at creation
    label TEXT NOT NULL,                  -- e.g. "my discord bot"
    scope TEXT NOT NULL DEFAULT 'send',   -- send-only in v1; field is future-proofed
    created_at INTEGER NOT NULL,
    last_used_at INTEGER,
    revoked_at INTEGER
);

CREATE TABLE sessions (                   -- web UI login sessions
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE TABLE server_config (              -- singleton row, admin-controlled
    id INTEGER PRIMARY KEY CHECK (id = 1),
    domain TEXT NOT NULL,
    signup_enabled BOOLEAN NOT NULL DEFAULT 1,
    dkim_selector TEXT NOT NULL DEFAULT 'default'
);

CREATE TABLE delivery_queue (
    id INTEGER PRIMARY KEY,
    from_user_id INTEGER NOT NULL REFERENCES users(id),
    recipient TEXT NOT NULL,
    message_path TEXT NOT NULL,           -- path to raw .eml on disk
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL,
    last_error TEXT,
    status TEXT NOT NULL DEFAULT 'pending' -- pending | delivered | dead
);
```

> **v4 forward-compat note (undecided):** v4 introduces multi-tenancy
> (multiple domains/orgs, 5GB hosted quotas). `server_config` as a singleton
> and no `org_id`/`tenant_id` anywhere means a real migration later. Cheap
> insurance now (nullable `org_id` columns, `domains` as a table instead of
> a singleton) vs. deliberately staying single-tenant and treating the
> migration as its own v4 design task — **not yet decided, revisit before
> scaffolding the schema for real.**

---

## 11. Inbound Mail (`stampd-engine::smtpd`, port 25)

- RFC 5321 command handling: HELO/EHLO, MAIL FROM, RCPT TO, DATA, RSET, QUIT
- STARTTLS (RFC 3207) — accept plaintext initially, upgrade
- Reject any `RCPT TO` not addressed to the server's configured domain
  (this *is* the anti-open-relay boundary)
- Basic SPF check on sender before accepting DATA (best-effort — Stampd
  doesn't control what Gmail/Outlook do on their end)
- No auth required for inbound (standard internet behavior)
- Deliver to local Maildir per user

## 12. Outbound Mail (`stampd-engine::submissiond`, port 587)

- Requires AUTH — send-only API tokens (hashed at rest, revocable) for
  programmatic/endpoint clients, or SMTP AUTH PLAIN/LOGIN over mandatory
  TLS for real mail clients (Thunderbird etc.), gated by per-user password
  (separate credential from the API token)
- DKIM-signs all outgoing mail
- Delivery: MX lookup → connect → retry with backoff → dead-letter after N
  attempts, tracked in `delivery_queue`
- Reputation matters here more than protocol correctness — rDNS on the
  sending IP should match the domain, or major providers will silently
  bin the mail

---

## 13. Auth Model (v1)

- **Signup:** open self-signup, admin can disable at any time
  (`server_config.signup_enabled`)
- **Tokens:** send-only, revocable, no inbox-read scope in v1 (`scope` field
  is future-proofed for later scopes)
- **Admin:** `is_admin` flag on `users` — single flag in v1, multi-role
  admin system deferred to v3

## 14. Admin Controls (v1 scope)

- Toggle self-signup on/off
- User management: list / disable / delete users
- Token management: view / revoke **any** user's tokens
- Domain config: the one served domain, DKIM selector/key rotation
- Mailbox size limits / message size limits
- Queue visibility: pending/dead-lettered outbound mail, manual retry/purge
- Basic delivery logs (accepted / rejected / bounced) — visibility only,
  no analytics in v1

---

## 15. Explicitly Out of Scope for v1

- IMAP/POP3 (mail client retrieval) — separate protocol/server, its own
  milestone, not bundled into v1
- DMARC enforcement
- Greylisting, spam scoring
- Multi-tenancy / hosted service (that's v4)
- Redis — SQLite handles v1's queue/session load; revisit only if
  greylisting/rate-limiting at scale becomes a real, measured need
- Encryption at rest (that's v2, and doing it "halfway" — e.g. encrypting
  at rest but decrypting server-side to serve the web UI — is explicitly
  rejected as not being zero-access at all, just disk encryption)

---

## 16. Open Decisions (not yet locked)

1. **v4 schema forward-compat** — add `org_id`/tenant scaffolding now at
   near-zero cost, or stay deliberately single-tenant and treat the v4
   migration as its own future design task?
2. Whether `stampd-gateway` and `stampd-admin` should get independent
   health-check/readiness endpoints now, even while co-deployed, to make
   the eventual "split into real microservices" path (if v4 ever needs it)
   less of a rewrite.

---

*This spec reflects everything locked through the current planning
conversation. Implementation has not started per project convention —
spec first, build on explicit go-ahead.*