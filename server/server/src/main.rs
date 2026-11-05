//! xatodo-server entry point
//!
//! Usage:
//! - `xatodo-server`: start the HTTP service (REST /api/v1 + MCP /mcp)
//!   - Standalone mode (default): Bearer apikey auth, listening on
//!     config.server.listen
//!   - Component mode (SL_ENDPOINT injected, launched by the superlive v2
//!     shell): token auth + single owner, see component.rs
//! - `xatodo-server migrate --from <sqlite-url> --to <pg-url>`: cross-database
//!   data migration
//!
//! Config: reads config.toml from the working directory by default; use the
//! XATODO_CONFIG env var to point elsewhere.
//! Logging: log facade + env_logger implementation (project discipline: app
//! code uses log:: macros uniformly, never tracing directly); default level
//! info, overridable via RUST_LOG.

mod api;
mod auth;
mod component;
mod config;
mod error;
mod mcp;
mod migrate_cmd;
mod router;
mod service;
mod state;
mod users_sync;

/// Single source of truth for the component id: the shell's dev anti-drift
/// check scans main.rs for `const COMPONENT_ID` (startup is refused when the
/// declared id and the source constant disagree); component/service modules
/// reference it via `crate::COMPONENT_ID`, and it doubles as the health loop
/// key (the shell matches manifest.id).
const COMPONENT_ID: &str = "xatodo";

use log::info;
use sea_orm::Database;
use xa_todo_migration::{Migrator, MigratorTrait};

use crate::config::Config;
use crate::state::AppState;

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
                    eprintln!("Usage: xatodo-server migrate --from <sqlite-url> --to <pg-url>");
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
            eprintln!("Usage: xatodo-server [migrate --from <sqlite-url> --to <pg-url>]");
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

/// Service startup flow:
/// load config → establish connection (ensure the parent dir exists for
/// sqlite) → run migrations → fork by mode:
/// - Component mode (SL_ENDPOINT injected): default owner + slsdk serve
///   (token auth/health/SIGTERM all handled by the SDK)
/// - Standalone mode: sync users → start axum (config.server.listen)
async fn serve() -> anyhow::Result<()> {
    // 1. Load config (component mode also needs database_url; users.entries
    //    is not consumed)
    let cfg = Config::load().await?;

    // 2. For sqlite, ensure the database parent directory exists
    //    (mode=rwc creates the file but not directories)
    ensure_sqlite_parent_dir(&cfg.server.database_url).await;

    // 3. Establish the database connection
    let db = Database::connect(&cfg.server.database_url).await?;

    // 4. Run migrations (idempotent; already-applied ones are skipped)
    Migrator::up(&db, None).await?;
    info!("Database migrations ready");

    // 5. Component-mode fork: launched by the shell; auth and listening are
    //    handled by slsdk serve
    if component::is_component_mode() {
        info!(
            "Component mode startup ({} injected)",
            slsdk_rs::ENV_ENDPOINT
        );
        return component::run(db).await;
    }

    // 6. Standalone mode: sync users (config.toml is the apikey authority)
    info!("Config loaded (listen = {})", cfg.server.listen);
    users_sync::sync_users(&db, &cfg.users.entries).await?;

    // 7. Start axum (REST + MCP, unified Bearer auth)
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

/// Ensure the parent directory of the sqlite database file exists
async fn ensure_sqlite_parent_dir(database_url: &str) {
    // Non-sqlite connections (postgres etc.) need no handling
    let Some(rest) = database_url.strip_prefix("sqlite://") else {
        return;
    };
    // Strip query params (e.g. ?mode=rwc)
    let path = rest.split('?').next().unwrap_or(rest);
    // In-memory DB / bare filename: no directory to create
    if path.is_empty() || path == ":memory:" {
        return;
    }
    if let Some(parent) = std::path::Path::new(path).parent() {
        if parent.as_os_str().is_empty() {
            return;
        }
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            log::warn!(
                "Failed to create database parent dir ({}): {e}",
                parent.display()
            );
        }
    }
}
