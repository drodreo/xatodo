//! xa-todo-daemon: the docker-resident service form (REST /api/v1 + MCP
//! /mcp + /events NDJSON stream, all behind Bearer apikey auth, optional SPA
//! static hosting via [server].web_root).
//!
//! Startup flow: load config → require database_url → connect → migrate →
//! users_sync (config [users.entries] is the apikey authority) → axum serve
//! on config.server.listen.
//! Event plane: the same slsdk notifier + integration adapter the component
//! forms use — every create/update/delete pushes one update_data_model line
//! to /events subscribers (the connect form's live-refresh bridge).
//!
//! Subcommand: `xa-todo-daemon migrate --from <sqlite-url> --to <pg-url>`
//! cross-database data migration.
//!
//! Config: reads config.toml from the working directory by default; use the
//! XATODO_CONFIG env var to point elsewhere.
//! Logging: log facade + env_logger implementation (project discipline: app
//! code uses log:: macros uniformly, never tracing directly); default level
//! info, overridable via RUST_LOG.

mod migrate_cmd;
mod web;

use axum::middleware;
use axum::Router;
use log::info;
use sea_orm::Database;
use slsdk_rs::A2uiNotifier;
use xa_todo_core::auth::auth_middleware;
use xa_todo_core::config::Config;
use xa_todo_core::router;
use xa_todo_core::state::AppState;
use xa_todo_core::users_sync;
use xa_todo_integration::notifier::sl_push_handle;
use xa_todo_migration::{Migrator, MigratorTrait};

#[tokio::main]
async fn main() {
    // Log init (shared by the serve / migrate paths)
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        // migrate subcommand: cross-database data migration
        Some("migrate") => {
            let (from, to) = match parse_migrate_args(&args) {
                Ok(v) => v,
                Err(msg) => {
                    eprintln!("{msg}");
                    eprintln!("Usage: xa-todo-daemon migrate --from <sqlite-url> --to <pg-url>");
                    std::process::exit(2);
                }
            };
            if let Err(e) = migrate_cmd::run(&from, &to).await {
                log::error!("Migration failed: {e:#}");
                std::process::exit(1);
            }
        }
        // Unknown subcommand
        Some(other) => {
            eprintln!("Unknown subcommand: {other}");
            eprintln!("Usage: xa-todo-daemon [migrate --from <sqlite-url> --to <pg-url>]");
            std::process::exit(2);
        }
        // Default: start the service
        None => {
            if let Err(e) = serve().await {
                log::error!("Service startup failed: {e:#}");
                std::process::exit(1);
            }
        }
    }
}

/// Service startup flow: load config (database_url required) → establish
/// connection (ensure the parent dir exists for sqlite) → run migrations →
/// sync users → start axum (config.server.listen, unified Bearer auth).
async fn serve() -> anyhow::Result<()> {
    // 1. Load config (users.entries is the apikey authority)
    let cfg = Config::load().await?;
    let database_url = cfg.require_database_url("daemon")?;

    // 2. For sqlite, ensure the database parent directory exists
    //    (mode=rwc creates the file but not directories)
    xa_todo_core::ensure_sqlite_parent_dir(&database_url).await;

    // 3. Establish the database connection
    let db = Database::connect(&database_url).await?;

    // 4. Run migrations (idempotent; already-applied ones are skipped)
    Migrator::up(&db, None).await?;
    info!("Database migrations ready");

    // 5. Sync users (config.toml is the apikey authority)
    info!("Config loaded (listen = {})", cfg.server.listen);
    users_sync::sync_users(&db, &cfg.users.entries).await?;

    // 6. Start axum (REST + MCP + /events, unified Bearer auth).
    //    Optional SPA static hosting ([server].web_root) rides on the same
    //    listener as a fallback service (daemon-only; absent field = unchanged
    //    API-only behavior).
    let notifier = A2uiNotifier::new();
    let app = web::attach(
        assemble_router(AppState::with_push(db, sl_push_handle(&notifier)), notifier),
        cfg.server.web_root.as_deref(),
    );
    let listener = tokio::net::TcpListener::bind(&cfg.server.listen).await?;
    info!("HTTP service started: http://{}", cfg.server.listen);
    axum::serve(listener, app).await?;
    Ok(())
}

/// Assembles the daemon router: business routes (REST + MCP) plus the
/// `/events` NDJSON stream, all behind the same Bearer apikey auth — the
/// connect form's bridge subscribes with its backend apikey exactly like the
/// REST forwarding path.
fn assemble_router(state: AppState, notifier: A2uiNotifier) -> Router {
    router::build(state.clone()).merge(
        slsdk_rs::events::events_router(notifier)
            .route_layer(middleware::from_fn_with_state(state, auth_middleware)),
    )
}

/// Parse migrate subcommand args (--from / --to)
fn parse_migrate_args(args: &[String]) -> Result<(String, String), String> {
    let mut from: Option<String> = None;
    let mut to: Option<String> = None;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--from" => {
                from = args.get(i + 1).cloned();
                i += 2;
            }
            "--to" => {
                to = args.get(i + 1).cloned();
                i += 2;
            }
            other => return Err(format!("Unknown argument: {other}")),
        }
    }
    match (from, to) {
        (Some(f), Some(t)) => Ok((f, t)),
        _ => Err("Missing --from or --to argument".to_string()),
    }
}

#[cfg(test)]
mod events_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request as HttpRequest, StatusCode};
    use futures_util::StreamExt;
    use tower::ServiceExt; // oneshot
    use xa_todo_core::config::UserEntry;
    use xa_todo_entity::user::UserRole;

    const KEY: &str = "test-apikey-events";

    /// Full daemon wiring against a throwaway sqlite database: migrations +
    /// one seeded apikey user + the same assemble_router the serve path uses.
    async fn test_app() -> Router {
        let dir = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("t.sqlite").display()
        );
        // The temp dir must outlive the sqlite connection; tests are
        // short-lived, so forgetting it for the process lifetime is fine.
        std::mem::forget(dir);
        let db = Database::connect(&url).await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        users_sync::sync_users(
            &db,
            &[UserEntry {
                name: "tester".into(),
                apikey: KEY.into(),
                role: UserRole::User,
            }],
        )
        .await
        .unwrap();
        let notifier = A2uiNotifier::new();
        assemble_router(AppState::with_push(db, sl_push_handle(&notifier)), notifier)
    }

    #[tokio::test]
    async fn events_requires_bearer_auth() {
        let app = test_app().await;
        let res = app
            .oneshot(HttpRequest::get("/events").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn create_pushes_update_line_to_events_stream() {
        let subscriber = test_app().await;
        let poster = subscriber.clone();

        // Subscribe first: oneshot returns after the handler registered its
        // notifier subscription (EventStream::spawn runs inside the handler),
        // so the create below cannot race it.
        let res = subscriber
            .oneshot(
                HttpRequest::get("/events")
                    .header(header::AUTHORIZATION, format!("Bearer {KEY}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let mut stream = res.into_body().into_data_stream();

        // Create one todo over the REST plane (same auth).
        let res = poster
            .oneshot(
                HttpRequest::post("/api/v1/todos")
                    .header(header::AUTHORIZATION, format!("Bearer {KEY}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{ "title": "events-contract" }"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        // The push must arrive as one NDJSON line on the stream.
        let mut line = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            line.push_str(&String::from_utf8_lossy(&chunk));
            if line.contains('\n') {
                break;
            }
        }
        let line = line.trim();
        assert!(!line.is_empty(), "no push arrived on /events");

        let msg: serde_json::Value = serde_json::from_str(line).unwrap();
        let upd = msg
            .get("updateDataModel")
            .expect("expected updateDataModel envelope");
        assert_eq!(upd["surfaceId"], "xatodo");
        assert_eq!(upd["path"], "/todos");
        let titles: Vec<&str> = upd["value"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["title"].as_str().unwrap())
            .collect();
        assert!(titles.contains(&"events-contract"));
    }
}
