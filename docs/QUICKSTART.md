# Stampd — Quick Start

A self-hosted, single-domain mail server for home labs and enterprises.

## Quick Install

### Option 1: Docker (Recommended)

```bash
# Clone the repository
git clone https://github.com/sabeeir/stampd.git
cd stampd

# Edit configuration
nano stampd.toml

# Start with Docker
docker-compose up -d

# Check status
docker-compose logs -f
```

### Option 2: Native Install

```bash
# Download and install
curl -fsSL https://sabeeir.qd.je/stampd/install.sh | sudo bash

# Configure
sudo nano /etc/stampd/stampd.toml

# Start
sudo systemctl start stampd
sudo systemctl enable stampd
```

### Option 3: Build from Source

```bash
# Clone and build
git clone https://github.com/sabeeir/stampd.git
cd stampd
cargo build --release

# Initialize configuration
./target/release/stampd init

# Edit configuration
nano stampd.toml

# Start
./target/release/stampd up
```

## Configuration

Edit `stampd.toml`:

```toml
[engine]
smtp_port = 25              # Inbound SMTP
submission_port = 587       # Outbound SMTP
domain = "mail.example.com" # Your domain

[gateway]
port = 8080                 # API port

[web]
port = 3000                 # Web UI port
```

## DNS Setup

Add these DNS records for your domain:

```
MX     mail.example.com  →  your-server-ip (priority 10)
TXT    mail.example.com  →  "v=spf1 ip4:your-server-ip ~all"
TXT    default._domainkey.mail.example.com  →  your-dkim-public-key
```

## First Run

1. Start Stampd: `stampd up`
2. Open web UI: `http://localhost:3000`
3. Create admin account
4. Send test email

## Commands

```bash
stampd up                    # Start all services
stampd up --only engine,gateway  # Start specific services
stampd down                  # Stop all services
stampd restart               # Restart all services
stampd status                # Show service status
stampd logs [service]        # View logs
stampd init                  # Initialize configuration
```

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

## Features

- **SMTP Server**: Inbound (port 25) + Submission (port 587)
- **DKIM Signing**: Automatic email authentication
- **SPF Validation**: Verify sender domains
- **Custom Domains**: Receive mail on your own domains
- **Web UI**: Modern, responsive interface
- **API**: RESTful API for third-party integrations
- **Filters**: Python-based filter system via Transit
- **Multi-user**: Individual mailboxes with quotas

## Home Lab Setup

For home labs with dynamic IPs:

1. Use a subdomain (e.g., `mail.home.example.com`)
2. Set up DynDNS or similar
3. Configure reverse DNS with your ISP
4. Use port 587 for submission (port 25 may be blocked)

## Enterprise Setup

For production environments:

1. Use Docker with Caddy reverse proxy
2. Enable TLS with Let's Encrypt
3. Configure monitoring and alerting
4. Set up regular backups
5. Review security hardening guide

## Troubleshooting

### Check service status
```bash
stampd status
```

### View logs
```bash
stampd logs engine
stampd logs gateway
```

### Test SMTP connection
```bash
telnet localhost 25
EHLO test
MAIL FROM:<test@example.com>
RCPT TO:<user@yourdomain.com>
```

### Verify DKIM
```bash
# Check DKIM key
cat /var/lib/stampd/dkim/default.txt

# Test DKIM signature
opendkim-testkey -d yourdomain.com -s default -vvv
```

## Support

- Documentation: https://sabeeir.qd.je/stampd
- Issues: https://github.com/sabeeir/stampd/issues
- Discord: [Join our community](https://discord.gg/stampd)

## License

MIT License - see [LICENSE](LICENSE) for details.
