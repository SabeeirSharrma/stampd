# Stampd

A self-hosted, lightweight, fast and single-domain mail server built for everyone, from home lab enthusiasts to enterprise teams.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.82+-orange.svg)](https://www.rust-lang.org/)
[![Node.js](https://img.shields.io/badge/Node.js-20+-green.svg)](https://nodejs.org/)

Powered by: [Transit](https://sabeeir.qd.je/transit) - by Sabeeir Sharrma

## What is Stampd?

Stampd is a complete SMTP mail server solution that lets you:

- **Receive email** from any sender (Gmail, Outlook, etc.) to your domain
- **Send email** as your domain's identity
- **Manage mailboxes** with a modern web interface
- **Filter spam** with customizable Python scripts
- **Integrate** with third-party tools via REST API

## Why Stampd?

### For Home Labs
- **Simple setup**: One command to start
- **Lightweight**: Runs on Raspberry Pi or old hardware
- **No vendor lock-in**: Your data stays on your server
- **Free**: Open source, no subscription fees

### For Enterprises
- **Production-ready**: Systemd integration, health checks, graceful shutdown
- **Scalable**: Handles thousands of users
- **Secure**: DKIM, SPF, TLS, security hardening
- **Compliant**: Full audit logging, data sovereignty

## Quick Start

```bash
# Docker (recommended)
git clone https://github.com/sabeeirsharrma/stampd.git
cd stampd
docker-compose up -d

# Or native install
curl -fsSL https://sabeeir.qd.je/stampd/install.sh | sudo bash
```

See [docs/QUICKSTART.md](docs/QUICKSTART.md) for detailed instructions.

## Launch from Source

### Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| **Rust** | 1.82+ | Engine + CLI |
| **Bun** | latest | Gateway + Web |
| **Python** | 3.11+ | Admin service |
| **pip** | — | Python packages |

### 1. Install dependencies

```bash
# Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Bun (if not installed)
curl -fsSL https://bun.sh/install | bash

# Python deps for admin service
cd stampd-admin
pip install -r <(python3 -c "
import tomllib
with open('pyproject.toml','rb') as f: d=tomllib.load(f)
print('\n'.join(d['project']['dependencies']))
")
cd ..

# Node deps for gateway + web
cd stampd-gateway && bun install && cd ..
cd stampd-web && bun install && cd ..
```

### 2. Configure

```bash
cargo run --bin stampd -- init
# Creates stampd.toml with sensible defaults.
# Edit it — at minimum set your domain under [engine].
```

### 3. Build

```bash
cargo build --release
```

This produces `target/release/stampd-engine` and `target/release/stampd`.

### 4. Initialize the database

```bash
# The engine creates the schema on first run, but if you want to
# initialize without starting SMTP, run the engine briefly:
./target/release/stampd-engine stampd.toml &
sleep 2
kill %1
```

### 5. Start services

**Option A — use the CLI supervisor (recommended):**

```bash
./target/release/stampd up
# Starts engine, gateway, admin, and web. Ctrl+C stops all.
```

**Option B — run each service individually:**

```bash
# Engine (SMTP + submission + queue)
STAMPD_DB_PATH=./data/stampd.db ./target/release/stampd-engine stampd.toml &

# Admin service (port 8081)
cd stampd-admin
STAMPD_DB_PATH=/full/path/to/data/stampd.db \
  python3 -m uvicorn app.main:app --host 0.0.0.0 --port 8081 &
cd ..

# Gateway (port 8080)
cd stampd-gateway
ADMIN_URL=http://127.0.0.1:8081 \
  bun run src/index.ts &
cd ..

# Web UI (port 3000)
cd stampd-web
bun run src/index.ts &
cd ..
```

### 6. Verify it's running

```bash
# Health checks
curl http://localhost:8081/health    # admin
curl http://localhost:8080/health    # gateway
curl http://localhost:3000           # web UI (open in browser)

# SMTP test
telnet localhost 25
# Should respond: 220 stampd ESMTP
```

### Port Reference

| Port | Protocol | Public? | Purpose |
|------|----------|---------|---------|
| **25** | SMTP | **Yes** | Inbound mail (other servers send to you) |
| **587** | SMTP | **Yes** | Submission (authenticated sending) |
| **8080** | HTTP | No | Gateway API (internal) |
| **8081** | HTTP | No | Admin API (internal) |
| **3000** | HTTP | No | Web UI (local browser) |

**Only ports 25 and 587 need to be forwarded** from your router/firewall
to your server's IP. The gateway, admin, and web UI are accessed locally.

If you're running from your local machine and only sending mail (not
receiving from external servers), you don't need to forward port 25 —
only 587 for submission.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      stampd-cli                              │
│                    (supervisor)                              │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │   Engine    │  │   Gateway   │  │    Web      │        │
│  │   (Rust)    │  │  (Node/TS)  │  │  (Astro)    │        │
│  │             │  │             │  │             │        │
│  │  SMTP/25    │  │  API/8080   │  │  UI/3000    │        │
│  │  SMTP/587   │  │             │  │             │        │
│  └─────────────┘  └─────────────┘  └─────────────┘        │
│         │               │               │                  │
│         └───────────────┴───────────────┘                  │
│                     Transit                                 │
│              (Python filter bridge)                         │
└─────────────────────────────────────────────────────────────┘
```

### Components

| Component | Language | Purpose |
|-----------|----------|---------|
| **Engine** | Rust | SMTP server, DKIM/SPF, mail storage |
| **Gateway** | Node.js/TypeScript | REST API, authentication, rate limiting |
| **Web** | Astro | Reference web interface |
| **CLI** | Rust | Process supervisor, management |
| **Filters** | Python | Spam filtering via Transit |

## Features

### Core
- SMTP server (inbound + submission)
- DKIM signing
- SPF validation
- Maildir storage
- Multi-user mailboxes
- Custom domains

### API & UI
- RESTful API
- Session + token authentication
- Modern web interface
- Real-time updates

### Operations
- Process supervision
- Health checks
- Graceful shutdown
- Log management
- Docker support
- Systemd integration

### Advanced
- Python filter system (via Transit)
- Rate limiting
- Quota management
- Admin API

## Configuration

Stampd is configured via `stampd.toml`:

```toml
[engine]
smtp_port = 25
submission_port = 587
domain = "mail.example.com"

[gateway]
port = 8080

[web]
port = 3000
```

See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) for all options.

## Development

See [Launch from Source](#launch-from-source) above for full setup instructions.

```bash
# Build + run with CLI supervisor
cargo build --release
./target/release/stampd up

# Or run individual services for development
STAMPD_DB_PATH=./data/stampd.db ./target/release/stampd-engine stampd.toml
```

## Deployment

### Docker
```bash
docker-compose up -d
```

### Systemd
```bash
sudo ./deploy/install.sh
sudo systemctl start stampd
```

### Manual
```bash
./target/release/stampd up
```

## Documentation

- [Quick Start](docs/QUICKSTART.md)
- [Configuration](docs/CONFIGURATION.md)
- [API Reference](docs/API.md)
- [Deployment Guide](docs/DEPLOYMENT.md)
- [Security Guide](docs/SECURITY.md)

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## Support

- **Issues**: [GitHub Issues](https://github.com/sabeeir/stampd/issues)
- **Discussions**: [GitHub Discussions](https://github.com/sabeeir/stampd/discussions)
- **Discord**: [Join our community](https://discord.gg/stampd)

## License

MIT License - see [LICENSE](LICENSE) for details.

---

Built with ❤️ by Sabeeir Sharrma

Powered by: [Transit](https://sabeeir.qd.je/transit) - by Sabeeir Sharrma
