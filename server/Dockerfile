# xatodo-server multi-stage build
# Build context = this directory (xatodo/server, the workspace root)
#
# Config mounting (pick one at container runtime):
#   1) mount to the default path: -v ./server/config.toml:/app/config.toml:ro
#   2) point via env var: -e XATODO_CONFIG=/app/config/config.toml
#      (matches the directory the config file is mounted into)
# Data mount (sqlite): -v ./data:/app/data

# ---- Build stage: compile the workspace (entity / migration / server) ----
FROM rust:1-slim AS builder
WORKDIR /build
COPY . .
# Build the server bin only (workspace deps entity / migration come along)
# The sqlite / rustls dependency chain is pure Rust — no extra system libs
RUN cargo build --release -p xa-todo-server

# ---- Runtime stage ----
FROM debian:bookworm-slim
# ca-certificates: required for postgres TLS connections (droppable for
# pure-sqlite deployments)
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/target/release/xatodo-server /app/xatodo-server

EXPOSE 8600
ENTRYPOINT ["/app/xatodo-server"]
