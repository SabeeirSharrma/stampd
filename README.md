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
git clone https://github.com/sabeeir/stampd.git
cd stampd
docker-compose up -d

# Or native install
curl -fsSL https://sabeeir.qd.je/stampd/install.sh | sudo bash
```

See [docs/QUICKSTART.md](docs/QUICKSTART.md) for detailed instructions.

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

```bash
# Clone and setup
git clone https://github.com/sabeeir/stampd.git
cd stampd

# Build
cargo build --release

# Run in development
cargo run --bin stampd -- up
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
