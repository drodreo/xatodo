# xa-todo release image (GHCR): daemon + web static hosting in one container.
# Build context = the repository root.
#
# The baked default /app/config.toml boots an out-of-the-box service: sqlite
# at /app/data/xatodo.db, API on 0.0.0.0:8600, and the bundled ui bundle
# served at / via [server].web_root. Mount a real config over
# /app/config.toml to declare apikey users ([users.entries]) — with the
# default config every API call is 401 until users are declared.
#
# Data mount (sqlite): -v ./data:/app/data
#
# The internal deployment form stays untouched: server/Dockerfile (API
# container with a mounted config).

# ---- Stage 1: daemon binary (workspace deps core/entity/migration come
#      along; zero superlive dependency) ----
FROM rust:1-slim AS rust-builder
WORKDIR /build
COPY server/ ./server/
COPY scripts/ ./scripts/
WORKDIR /build/server
# The sqlite / rustls dependency chain is pure Rust — no extra system libs
RUN cargo build --release -p xa-todo-daemon

# ---- Stage 2: ui bundle (the superlive component form: entry.js + assets).
#      Note: the bundle targets the superlive shell environment and ships no
#      index.html — a browser hitting / gets a 404 (see README); the API on
#      the same listener is fully functional. ----
FROM node:24-alpine AS ui-builder
WORKDIR /build
COPY ui/ ./ui/
# pnpm via corepack; frozen install stays reproducible against the committed
# lockfile (allowBuilds in pnpm-workspace.yaml declares the build policy)
RUN corepack enable && corepack prepare pnpm@12 --activate \
    && cd ui && pnpm install --frozen-lockfile && pnpm build

# ---- Runtime stage ----
FROM debian:bookworm-slim
# ca-certificates: required for postgres TLS connections (droppable for
# pure-sqlite deployments)
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=rust-builder /build/server/target/release/xa-todo-daemon /app/xa-todo-daemon
COPY --from=ui-builder /build/ui/dist /app/web
# Baked out-of-the-box config (mount over /app/config.toml to override)
COPY <<'EOF' /app/config.toml
database_url = "sqlite://data/xatodo.db?mode=rwc"

[server]
listen = "0.0.0.0:8600"
# Bundled ui bundle served as the SPA frontend on the same listener
web_root = "/app/web"

[users]
entries = []
EOF
ENV XATODO_CONFIG=/app/config.toml

EXPOSE 8600
ENTRYPOINT ["/app/xa-todo-daemon"]
