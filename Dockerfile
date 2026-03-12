# =============================================================================
# SPINE Multi-Stage Dockerfile
# =============================================================================
# Stage 1: Build all binaries
# Stage 2: Minimal runtime image
# =============================================================================

# Build stage
FROM rust:1.82-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies first
COPY Cargo.toml Cargo.lock* ./
COPY src/spine-core/Cargo.toml src/spine-core/Cargo.toml
COPY src/spine-parser/Cargo.toml src/spine-parser/Cargo.toml
COPY src/spine-protocol/Cargo.toml src/spine-protocol/Cargo.toml
COPY src/spine-agent/Cargo.toml src/spine-agent/Cargo.toml
COPY src/spine-compiler/Cargo.toml src/spine-compiler/Cargo.toml
COPY src/spine-wasm/Cargo.toml src/spine-wasm/Cargo.toml
COPY src/spine-cluster/Cargo.toml src/spine-cluster/Cargo.toml
COPY src/spine-neural/Cargo.toml src/spine-neural/Cargo.toml
COPY src/spine-crypto/Cargo.toml src/spine-crypto/Cargo.toml
COPY src/spine-human/Cargo.toml src/spine-human/Cargo.toml
COPY src/spine-browser/Cargo.toml src/spine-browser/Cargo.toml
COPY src/spine-agentic/Cargo.toml src/spine-agentic/Cargo.toml
COPY src/spine-transport/Cargo.toml src/spine-transport/Cargo.toml
COPY src/spine-stream/Cargo.toml src/spine-stream/Cargo.toml
COPY src/spine-recursive/Cargo.toml src/spine-recursive/Cargo.toml
COPY src/spine-knowledge/Cargo.toml src/spine-knowledge/Cargo.toml
COPY src/spine-kernel/Cargo.toml src/spine-kernel/Cargo.toml
COPY src/spine-cli/Cargo.toml src/spine-cli/Cargo.toml
COPY src/spine-gateway/Cargo.toml src/spine-gateway/Cargo.toml

# Create stub lib.rs files for dependency caching
RUN for dir in src/spine-*/; do \
    mkdir -p "$dir/src"; \
    echo "" > "$dir/src/lib.rs"; \
    done && \
    # Create stub main.rs for binary crates
    echo "fn main() {}" > src/spine-core/src/main.rs && \
    echo "fn main() {}" > src/spine-cli/src/main.rs && \
    echo "fn main() {}" > src/spine-gateway/src/main.rs && \
    echo "fn main() {}" > src/spine-browser/src/main.rs

# Build dependencies (cached layer)
RUN cargo build --release --workspace 2>/dev/null || true

# Copy actual source
COPY . .

# Touch all source files to invalidate stubs
RUN find src/spine-*/src -name "*.rs" -exec touch {} +

# Build release binaries
RUN cargo build --release \
    -p spine-core \
    -p spine-cli \
    -p spine-gateway

# =============================================================================
# Runtime stage
# =============================================================================
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -r -s /bin/false spine

WORKDIR /app

# Copy binaries
COPY --from=builder /build/target/release/spine-core /usr/local/bin/spine-core
COPY --from=builder /build/target/release/spine /usr/local/bin/spine
COPY --from=builder /build/target/release/spine-gateway /usr/local/bin/spine-gateway

# Create directories
RUN mkdir -p /app/sessions /app/certs && chown -R spine:spine /app

# Default config
COPY <<'EOF' /app/spine.toml
[server]
host = "0.0.0.0"
port = 8080
metrics_port = 9090
max_sessions = 1000
idle_timeout_secs = 300

[tls]
enabled = false

[logging]
format = "json"
level = "info"
EOF

USER spine

# Expose ports: TCP, WebSocket, QUIC, Metrics, Gateway
EXPOSE 8080 8081 8082 9090 9091

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD spine status 127.0.0.1:8080 2>/dev/null || exit 1

ENTRYPOINT ["spine-core"]
