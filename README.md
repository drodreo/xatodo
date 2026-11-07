# xatodo

A multi-user todo service. Rust + axum + sea-orm, dual REST and MCP entries, Bearer apikey authentication, per-user data isolation — and a [superlive](https://github.com/drodreo/superlive) component (surface UI + lifecycle integration) on top of the same backend.

## Directory structure

```
xatodo/
├── compose.yml             # deployment (traefik /backends/xatodo)
├── Dockerfile              # release image (daemon + web static hosting in one container; GHCR)
├── manifest.json           # superlive component declaration
├── scripts/                # component build / packaging helpers
│   ├── package-standalone.sh  # builds dist/xatodo-standalone-<version>-<platform>.zip
│   └── package-connect.sh     # builds dist/xatodo-connect-<version>-<platform>.zip
├── ui/                     # component UI bundle source (React; mounts as the superlive surface)
└── server/                 # workspace root
    ├── Cargo.toml          # workspace definition (7 members, see below)
    ├── Dockerfile          # builds xa-todo-daemon (the docker-resident form)
    ├── config.example.toml # config template (real config.toml is gitignored)
    ├── entity/             # sea-orm entities (xa-todo-entity)
    ├── migration/          # database migrations (xa-todo-migration)
    ├── core/               # business core (xa-todo-core): REST / MCP / service /
    │                       # auth / users sync / config — zero host dependency
    ├── integration/        # superlive integration (xa-todo-integration):
    │                       # component id / owner attribution / manifest /
    │                       # event-plane adapter
    ├── daemon/             # xa-todo-daemon (bin): docker-resident service
    ├── standalone/         # xa-todo-standalone (bin): superlive shell component
    └── connect/            # xa-todo-connect (bin): daemon connector for the shell
```

## The three server forms

All three share the business core (`xa-todo-core`); a form is just a way of
wiring the core:

- **daemon** (`xa-todo-daemon`): the docker-resident service. Bearer apikey
  auth ([users.entries] synced into the users table), axum on
  `config.server.listen`. Optional SPA static hosting: set `[server].web_root`
  and any GET that misses the API routes is answered from that directory
  (missing files fall back to index.html). Zero superlive dependency. This is
  the form the Dockerfiles / compose deploy.
- **standalone** (`xa-todo-standalone`): launched by the superlive shell.
  Single local owner replaces apikey auth (the shell gateway + slsdk token
  wall handle authentication), slsdk serves on `SL_ENDPOINT` (uds/tcp) with
  `/health` + `/events`; data changes push to shell surfaces live.
- **connect** (`xa-todo-connect`): a pure forwarder for the shell in front
  of a running daemon — no local database. REST (/api/v1) and MCP (/mcp)
  requests pass through to `[backend].endpoint` with the backend apikey;
  the daemon's `/events` stream feeds the shell surfaces. Configured via
  `[backend]` in config.toml.

## Run locally

```bash
cd server
cp config.example.toml config.toml   # edit the apikey
cargo run -p xa-todo-daemon
```

The daemon listens on `0.0.0.0:8600` with a default database of
`sqlite://data/xatodo.db?mode=rwc` (both editable in config.toml; the
database URL also accepts postgres).

The frontend lives in `ui/` (React + Vite + pnpm): `pnpm install && pnpm build`, or `pnpm dev` for the dev server.

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

- Routing: traefik `PathPrefix(/backends/xatodo)` + stripprefix
- Data: `./data` mounted at `/app/data` (sqlite database files)
- Config: `./server/config.toml` mounted read-only, resolved via `XATODO_CONFIG`

## Cross-database migration

```bash
xa-todo-daemon migrate --from sqlite://data/xatodo.db?mode=rwc --to postgres://user:pass@host/xatodo
```

Flow: read all users + todos from the source database → run migrations on the target to create tables → insert row by row (UUID/timestamps handled by sea-orm). Idempotent: records with existing ids are skipped, safe to re-run.

## Release artifacts (tag-triggered)

Pushing a `v*` tag runs [.github/workflows/release.yml](.github/workflows/release.yml), which publishes:

- **Docker image**: `ghcr.io/drodreo/xatodo` (tagged with the version without the `v` prefix, plus `latest`) — daemon and ui bundle in one container. Out of the box:

  ```bash
  docker run -d -p 8600:8600 -v ./data:/app/data ghcr.io/drodreo/xatodo
  ```

  The baked default config serves sqlite at `/app/data/xatodo.db`, the API on `0.0.0.0:8600`, and the bundled ui bundle at `/` (`[server].web_root`). Mount a real config to declare apikey users — without `[users.entries]` every API call is 401:

  ```bash
  docker run -d -p 8600:8600 \
    -v ./data:/app/data \
    -v ./config.toml:/app/config.toml:ro \
    ghcr.io/drodreo/xatodo
  ```

  Note on the bundled web bundle: it is the superlive component build (entry.js + surface assets, no index.html — the UI targets the shell environment). A browser hitting `/` gets a 404 until a standalone web form exists; the API (`/api/v1`, `/mcp`) is fully functional regardless.
- **Component packages**: `xatodo-standalone-<version>-<platform>.zip` and `xatodo-connect-<version>-<platform>.zip` are attached to the GitHub Release, built for all three shell platforms — `linux-x86_64`, `macos-aarch64` (Apple Silicon), `windows-x86_64`; import the package matching your platform. On Windows the server binary ships as `bin/xa-todo-<form>.exe` and the manifest launch command references it accordingly. Both packages carry the same manifest id (`xatodo`) — the component's runtime self-reported identity — so they are two mutually exclusive forms of one component: installing one over the other replaces it (the shell preserves the user-edited `config.toml` on replacement). The connect package declares `backend.endpoint` / `backend.apikey` as required config fields — the shell gates the first start on them and renders the daemon settings right on the component config page; its display name carries the form marker. It also declares the optional boolean `backend.use_system_proxy` (default `false`): direct reach ignores ambient proxy settings (`http_proxy` env) so intranet daemons are not silently rerouted through an unreachable proxy — enable it only when the network path to the daemon requires a proxy.

## superlive component integration

This project is also a [superlive](https://github.com/drodreo/superlive) component:

- `manifest.json` declares the component (id, namespace, entry points, menu entry)
- `ui/public/surface.json` provides the declarative surface UI; the same React tree mounts as a surface inside the superlive shell (the bundle targets the shell environment — it has no standalone web form)
- `scripts/superlive.build` is the component build entry the shell invokes; `scripts/gen-manifest.py`, `scripts/package-standalone.sh` and `scripts/package-connect.sh` handle packaging
- The backend talks to the shell over slsdk-rs (spawn env + `/health` + `/events` push channel), so todos update live inside the shell

The `slsdk-rs` / `slsdk-ts` dependencies are pinned to a superlive commit, so a plain `cargo check` / `pnpm install` works without a local superlive checkout.
