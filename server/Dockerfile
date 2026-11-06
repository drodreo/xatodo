# xa-todo-daemon multi-stage build (the docker-resident service form)
# Build context = this directory (xatodo/server, the workspace root)
#
# Config mounting (pick one at container runtime):
#   1) mount to the default path: -v ./server/config.toml:/app/config.toml:ro
#   2) point via env var: -e XATODO_CONFIG=/app/config/config.toml
#      (matches the directory the config file is mounted into)
# Data mount (sqlite): -v ./data:/app/data

# ---- Build stage: compile the daemon bin (workspace deps core/entity/
#      migration come along; zero superlive dependency) ----
FROM rust:1-slim AS builder
WORKDIR /build
COPY . .
# The sqlite / rustls dependency chain is pure Rust — no extra system libs
RUN cargo build --release -p xa-todo-daemon

# ---- Runtime stage ----
FROM debian:bookworm-slim
# ca-certificates: required for postgres TLS connections (droppable for
# pure-sqlite deployments)
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/target/release/xa-todo-daemon /app/xa-todo-daemon

EXPOSE 8600
ENTRYPOINT ["/app/xa-todo-daemon"]
