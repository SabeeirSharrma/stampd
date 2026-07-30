# ── Stage 1: Build Rust binaries ──────────────────────────────
FROM rust:1.82-bookworm AS rust-builder

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY stampd-engine/ stampd-engine/
COPY stampd-cli/ stampd-cli/

# Build release binaries
RUN cargo build --release --bin stampd-engine --bin stampd

# ── Stage 2: Build Node gateway ──────────────────────────────
FROM oven/bun:1.3-bookworm AS node-builder

WORKDIR /app

# Copy gateway source
COPY stampd-gateway/ stampd-gateway/

# Install dependencies and build
WORKDIR /app/stampd-gateway
RUN bun install --frozen-lockfile

# ── Stage 3: Runtime image ──────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    python3 \
    python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Create directories
RUN mkdir -p /var/lib/stampd/mail \
    /var/lib/stampd/dkim \
    /var/lib/stampd/filters \
    /var/run/stampd \
    /var/log/stampd \
    /etc/stampd \
    /app/stampd-gateway/python/filters

# Copy Rust binaries
COPY --from=rust-builder /app/target/release/stampd-engine /usr/local/bin/
COPY --from=rust-builder /app/target/release/stampd /usr/local/bin/

# Copy Node gateway
COPY --from=node-builder /app/stampd-gateway/ /app/stampd-gateway/

# Copy default config
COPY stampd.toml /etc/stampd/stampd.toml

# Copy Transit Python runtime
COPY transit/packages/transit-py-runtime/transit_server.py /app/stampd-gateway/python/filters/

# Set working directory
WORKDIR /app

# Expose ports
# 25: SMTP (inbound)
# 587: Submission (outbound)
# 8080: Gateway API
# 3000: Web UI (optional, can be served by reverse proxy)
EXPOSE 25 587 8080 3000

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Default command
CMD ["stampd", "up"]
