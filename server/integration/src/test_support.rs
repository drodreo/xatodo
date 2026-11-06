//! Test-only helpers shared by the integration tests: a temp sqlite file DB
//! that is migration-ready (under /tmp, deleted when the test finishes).

use std::path::PathBuf;

use sea_orm::DatabaseConnection;
use uuid::Uuid;
use xa_todo_migration::{Migrator, MigratorTrait};

/// Sets up a temp sqlite file DB that is migration-ready.
pub(crate) async fn test_db(tag: &str) -> (DatabaseConnection, PathBuf) {
    let path =
        std::env::temp_dir().join(format!("xatodo-component-test-{tag}-{}.db", Uuid::new_v4()));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let db = sea_orm::Database::connect(&url)
        .await
        .expect("connect test db");
    Migrator::up(&db, None).await.expect("migrate test db");
    (db, path)
}

/// Deletes the test DB file (unlink works even with the connection open;
/// on failure /tmp self-cleaning is the fallback).
pub(crate) fn cleanup(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}
