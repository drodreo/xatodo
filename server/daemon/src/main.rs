//! xa-todo-daemon: the docker-resident service form (REST /api/v1 + MCP
//! /mcp behind Bearer apikey auth).
//!
//! Startup flow: load config → require database_url → connect → migrate →
//! users_sync (config [users.entries] is the apikey authority) → axum serve
//! on config.server.listen.
//! Zero superlive dependency: no component SDK and no integration layer —
//! data-change pushes are wired as a silent drop (the daemon has no
//! event-plane consumers).
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

use log::info;
use sea_orm::Database;
use xa_todo_core::config::Config;
use xa_todo_core::router;
use xa_todo_core::state::AppState;
use xa_todo_core::users_sync;
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

    // 6. Start axum (REST + MCP, unified Bearer auth); pushes drop silently
    let app = router::build(AppState::new(db));
    let listener = tokio::net::TcpListener::bind(&cfg.server.listen).await?;
    info!("HTTP service started: http://{}", cfg.server.listen);
    axum::serve(listener, app).await?;
    Ok(())
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
