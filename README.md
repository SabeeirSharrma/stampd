# Stampd

A self-hosted, single-domain mail server. Receives inbound mail from anyone,
sends outbound mail only as that domain's identity. Multi-user, with individual
mailboxes, self-signup (admin-revocable), and both a reference web UI and a
public API for third-party UI/tooling.

**Status:** v0.1.0 — scaffolding phase

## Architecture

```
stampd/
├── stampd-engine/     (Rust)   — SMTP core, trust boundary
├── stampd-gateway/    (Node/TS) — Public API surface
├── stampd-admin/      (Python) — Business/admin logic
├── stampd-filters/    — User-defined hook scripts
├── stampd-web/        (TS)    — Reference web UI
└── stampd-cli/        (Rust)  — Process supervisor
```

## Quick Start

```bash
# Build Rust crates
cargo build

# Start all services
cargo run -p stampd-cli -- up

# Or start specific services
cargo run -p stampd-cli -- up --only engine,gateway
```

## Configuration

Edit `stampd.toml` to configure services. See [spec.md](spec.md) for full
configuration reference.

## Development

```bash
# Rust (engine + CLI)
cargo check
cargo test
cargo clippy

# Gateway (Node/TS)
cd stampd-gateway && bun install && bun run dev

# Admin (Python)
cd stampd-admin && pip install -e ".[dev]" && uvicorn app.main:app --reload
```

## Documentation

- [Project Spec](spec.md) — full specification
- [Development Plan](PLAN.md) — implementation roadmap
- [Failure Modes](docs/failure-modes.md) — failure mode contract

## License

MIT
