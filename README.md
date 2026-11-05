# xatodo

A multi-user todo service. Rust + axum + sea-orm, dual REST and MCP entries, Bearer apikey authentication, per-user data isolation — and a [superlive](https://github.com/drodreo/superlive) component (surface UI + lifecycle integration) on top of the same backend.

## Directory structure

```
xatodo/
├── compose.yml             # deployment (traefik /backends/xatodo + /apps/xatodo)
├── manifest.json           # superlive component declaration
├── scripts/                # component build / packaging helpers
├── web/                    # React frontend (also mounts as the superlive surface)
└── server/                 # workspace root
    ├── Cargo.toml          # workspace definition (entity / migration / server)
    ├── Dockerfile
    ├── config.example.toml # config template (real config.toml is gitignored)
    ├── entity/             # sea-orm entities (xa-todo-entity)
    ├── migration/          # database migrations (xa-todo-migration)
    └── server/             # service implementation (xa-todo-server, bin: xatodo-server)
```

## Run locally

```bash
cd server
cp config.example.toml config.toml   # edit the apikey
cargo run -p xa-todo-server
```

The server listens on `0.0.0.0:8600` with a default database of `sqlite://data/xatodo.db?mode=rwc` (both editable in config.toml; the database URL also accepts postgres).

The frontend lives in `web/` (React + Vite + pnpm): `pnpm install && pnpm build`, or `pnpm dev` for the dev server.

## Authentication

Every endpoint (REST and MCP) requires Bearer auth:

```
Authorization: Bearer <apikey>
```

Apikeys live in `[users.entries]` in config.toml and are synced into the users table at startup (new names are inserted; existing names get their apikey/role updated).

## REST API (/api/v1)

| Method | Path | Description |
|---|---|---|
| GET | /api/v1/todos?completed=&limit=&offset= | list (optional filter/pagination) |
| POST | /api/v1/todos | create `{title, notes?, due_at?}` |
| GET | /api/v1/todos/{id} | detail |
| PATCH | /api/v1/todos/{id} | update `{title?, notes?, completed?, due_at?}` |
| DELETE | /api/v1/todos/{id} | delete |
| GET | /api/v1/me | current user info |

Data is isolated per user: you can only read and write your own todos. `due_at` takes RFC3339 strings.

## MCP (/mcp)

Streamable HTTP MVP shape: POST, one request one response (JSON). Supported methods: initialize, notifications/initialized, tools/list, tools/call, ping.

Tools: `todo_create(title, notes?, due_at?)` / `todo_list(completed?, limit?)` / `todo_update(todo_id, title?, notes?, completed?, due_at?)` / `todo_delete(todo_id)` — same permission model as REST (shared auth middleware, per-user isolation).

## Deploy with compose

```bash
cp server/config.example.toml server/config.toml   # edit the apikey
docker compose up -d --build
```

- Routing: traefik `PathPrefix(/backends/xatodo)` + stripprefix; the web container mounts at `/apps/xatodo`
- Data: `./data` mounted at `/app/data` (sqlite database files)
- Config: `./server/config.toml` mounted read-only, resolved via `XATODO_CONFIG`

## Cross-database migration

```bash
xatodo-server migrate --from sqlite://data/xatodo.db?mode=rwc --to postgres://user:pass@host/xatodo
```

Flow: read all users + todos from the source database → run migrations on the target to create tables → insert row by row (UUID/timestamps handled by sea-orm). Idempotent: records with existing ids are skipped, safe to re-run.

## superlive component integration

This project is also a [superlive](https://github.com/drodreo/superlive) component:

- `manifest.json` declares the component (id, namespace, entry points, menu entry)
- `web/public/surface.json` provides the declarative surface UI; the same React tree mounts as a surface inside the superlive shell or standalone via nginx
- `scripts/superlive.build` is the component build entry the shell invokes; `scripts/gen-manifest.py` and `scripts/package-component.sh` handle packaging
- The backend talks to the shell over slsdk-rs (spawn env + `/health` + `/events` push channel), so todos update live inside the shell

The `slsdk-rs` / `slsdk-ts` dependencies are pinned to a superlive commit, so a plain `cargo check` / `pnpm install` works without a local superlive checkout.
