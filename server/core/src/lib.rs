//! xa-todo-core: the xatodo business core (pure business, no launch form).
//!
//! REST handlers (/api/v1) + MCP endpoint (/mcp) + the service layer + the
//! standalone-form auth middleware + users sync + the config schema. Every
//! binary form (daemon / standalone / connect) assembles on top of this
//! crate; host-specific machinery never lives here — superlive integration
//! (component id, owner attribution, manifest, event-plane adapter) lives in
//! [`xa-todo-integration`].
//!
//! Data-change push: after every successful create/update/delete the service
//! layer fetches the latest full list and hands the JSON over to the push
//! handle on [`state::AppState`]. What happens to it (superlive event plane
//! / silent drop) is decided by the form's wiring, never by the core.

pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod mcp;
pub mod router;
pub mod service;
pub mod state;
pub mod users_sync;

/// Ensures the parent directory of the sqlite database file exists.
///
/// Non-sqlite connections (postgres etc.) need no handling. Shared by every
/// DB-backed form (daemon / standalone): `mode=rwc` creates the file but not
/// directories.
pub async fn ensure_sqlite_parent_dir(database_url: &str) {
    // Strip the scheme prefix
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
