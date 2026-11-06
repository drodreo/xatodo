//! xa-todo-standalone: the form launched by the superlive v2 shell under the
//! B-plane contract.
//!
//! Startup flow: load config → require database_url → connect → migrate →
//! ensure the single local owner → assemble the component router (owner
//! injection replaces apikey auth — the shell gateway has already validated
//! the per-component token, and slsdk serve re-validates SL_TOKEN as the
//! first wall) → slsdk serve_with_events (endpoint from SL_ENDPOINT, uds/tcp,
//! graceful SIGTERM shutdown, the UDS socket file is cleaned up on exit).
//!
//! Differences from the daemon form:
//! - Auth: no apikey middleware; all requests are attributed to the single
//!   owner (users table row name="local", UUID primary key); users_sync is
//!   skipped (config.toml's [users.entries] is the daemon-form authority).
//! - Listening: SL_ENDPOINT is parsed and bound by slsdk serve;
//!   config.server.listen is unused. config.toml must still exist and stays
//!   parse-rigid (listen + [users] placeholders are required by
//!   deserialization; the minimal component config carries listen +
//!   database_url + users.entries=[]).
//! - Identity: COMPONENT_ID (integration crate) doubles as the health loop
//!   key and the registry dispatch key (namespace) — the manifest generated
//!   by the packaging script must be derived from the same constant.
//!
//! The data layer (entity/migration/service) and the REST/MCP routes are
//! fully reused with zero changes (xa-todo-core); under shell forwarding
//! semantics the component receives its own complete path space
//! (/api/v1/todos, /mcp).

use log::info;
use sea_orm::Database;
use slsdk_rs::A2uiNotifier;
use xa_todo_core::config::Config;
use xa_todo_core::state::AppState;
use xa_todo_integration as integration;
use xa_todo_migration::{Migrator, MigratorTrait};

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(e) = run().await {
        log::error!("Service startup failed: {e:#}");
        std::process::exit(1);
    }
}

/// Component-form startup: load config → migrate → ensure the default owner
/// → assemble routes → slsdk serve.
/// Blocks until graceful shutdown (SIGTERM is handled by serve's built-in
/// listener; the UDS socket file is cleaned up on exit).
///
/// serve_with_events: mounts the standard /events endpoint (NDJSON push
/// stream) sharing the same notifier instance the core's push handle
/// forwards into — the service-layer push after create/update/delete reaches
/// all subscribers through it (shell bridge → live refresh on every surface).
async fn run() -> anyhow::Result<()> {
    let cfg = Config::load().await?;
    let database_url = cfg.require_database_url("standalone")?;
    xa_todo_core::ensure_sqlite_parent_dir(&database_url).await;

    let db = Database::connect(&database_url).await?;
    Migrator::up(&db, None).await?;
    info!("Database migrations ready");

    let owner = integration::owner::ensure_local_owner(&db).await?;
    info!("Standalone: default owner ready (id = {})", owner.id);

    let notifier = A2uiNotifier::new();
    let state = AppState::with_push(db, integration::notifier::sl_push_handle(&notifier));
    let app = integration::router::build_component_router(state, owner);
    let manifest = integration::make_manifest();
    let serve_info = slsdk_rs::serve_with_events(app, &manifest, None, notifier).await?;
    info!("Component service started: {}", serve_info.endpoint);

    // serve's return value: awaits completion of the accept/serve loop
    // (returns after graceful shutdown)
    serve_info.wait().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use slsdk_rs::Endpoint;

    #[test]
    fn endpoint_parsing() {
        // Pin down the component side's understanding of B-plane contract
        // descriptors: uds / tcp / invalid, three forms
        let ep: Endpoint = "uds:/tmp/xatodo.sock".parse().unwrap();
        assert_eq!(
            ep,
            Endpoint::Uds {
                path: "/tmp/xatodo.sock".to_string()
            }
        );

        let ep: Endpoint = "tcp:127.0.0.1:39877".parse().unwrap();
        assert_eq!(
            ep,
            Endpoint::Tcp {
                addr: "127.0.0.1:39877".parse().unwrap()
            }
        );

        assert!("xatodo.sock".parse::<Endpoint>().is_err());
    }
}
