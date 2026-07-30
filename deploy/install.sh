#!/bin/bash
# Stampd Installation Script
# Supports: Ubuntu 22.04+, Debian 12+, Fedora 38+, CentOS Stream 9+

set -e

STAMPD_VERSION="0.1.0"
INSTALL_DIR="/opt/stampd"
DATA_DIR="/var/lib/stampd"
CONFIG_DIR="/etc/stampd"
LOG_DIR="/var/log/stampd"
RUN_DIR="/var/run/stampd"

echo "Stampd Mail Server Installer v${STAMPD_VERSION}"
echo "================================================"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "Error: Please run as root (sudo ./install.sh)"
    exit 1
fi

# Detect package manager
if command -v apt-get &> /dev/null; then
    PKG_MANAGER="apt"
elif command -v dnf &> /dev/null; then
    PKG_MANAGER="dnf"
elif command -v yum &> /dev/null; then
    PKG_MANAGER="yum"
else
    echo "Error: Unsupported package manager. Install manually."
    exit 1
fi

echo "Detected package manager: ${PKG_MANAGER}"
echo ""

# Install dependencies
echo "Installing dependencies..."
if [ "$PKG_MANAGER" = "apt" ]; then
    apt-get update
    apt-get install -y curl ca-certificates python3 python3-pip
elif [ "$PKG_MANAGER" = "dnf" ]; then
    dnf install -y curl ca-certificates python3 python3-pip
elif [ "$PKG_MANAGER" = "yum" ]; then
    yum install -y curl ca-certificates python3 python3-pip
fi

# Install Bun (for gateway)
echo "Installing Bun..."
curl -fsSL https://bun.sh/install | bash
export BUN_INSTALL="$HOME/.bun"
export PATH="$BUN_INSTALL/bin:$PATH"

# Install Rust (for building from source)
echo "Installing Rust..."
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Create directories
echo "Creating directories..."
mkdir -p "$INSTALL_DIR" "$DATA_DIR/mail" "$DATA_DIR/dkim" "$DATA_DIR/filters" \
         "$CONFIG_DIR" "$LOG_DIR" "$RUN_DIR"

# Download or build Stampd
echo "Installing Stampd..."
if command -v stampd &> /dev/null; then
    echo "Stampd already installed. Updating..."
    # In production, download release binary here
    # For now, build from source
fi

# Build from source
TEMP_DIR=$(mktemp -d)
cd "$TEMP_DIR"

echo "Cloning repository..."
git clone https://github.com/sabeeir/stampd.git
cd stampd

echo "Building..."
cargo build --release

# Install binaries
cp target/release/stampd-engine "$INSTALL_DIR/"
cp target/release/stampd "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/stampd-engine" "$INSTALL_DIR/stampd"

# Create symlink
ln -sf "$INSTALL_DIR/stampd" /usr/local/bin/stampd

# Copy default config
if [ ! -f "$CONFIG_DIR/stampd.toml" ]; then
    cp stampd.toml "$CONFIG_DIR/stampd.toml"
    echo "Created default configuration at $CONFIG_DIR/stampd.toml"
fi

# Copy gateway files
cp -r stampd-gateway "$INSTALL_DIR/"
cd "$INSTALL_DIR/stampd-gateway"
bun install --frozen-lockfile

# Copy Python filter runtime
mkdir -p "$INSTALL_DIR/python/filters"
cp "$TEMP_DIR/stampd/transit/packages/transit-py-runtime/transit_server.py" \
   "$INSTALL_DIR/python/filters/"

# Create systemd service
cp "$TEMP_DIR/stampd/deploy/systemd/stampd.service" /etc/systemd/system/
systemctl daemon-reload

# Create user
if ! id -u stampd &>/dev/null; then
    useradd -r -s /bin/false -d "$DATA_DIR" stampd
fi

# Set permissions
chown -R stampd:stampd "$DATA_DIR" "$LOG_DIR" "$RUN_DIR"
chmod 750 "$DATA_DIR" "$LOG_DIR" "$RUN_DIR"

# Cleanup
cd /
rm -rf "$TEMP_DIR"

echo ""
echo "Installation complete!"
echo ""
echo "Next steps:"
echo "  1. Edit configuration: sudo nano $CONFIG_DIR/stampd.toml"
echo "  2. Start Stampd: sudo systemctl start stampd"
echo "  3. Enable on boot: sudo systemctl enable stampd"
echo "  4. Check status: sudo stampd status"
echo ""
echo "Web UI: http://localhost:8080"
echo "Logs: sudo journalctl -u stampd -f"
echo ""
