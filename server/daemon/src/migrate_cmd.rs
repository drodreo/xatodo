//! migrate subcommand: cross-database data migration
//!
//! Usage: `xa-todo-daemon migrate --from <sqlite-url> --to <pg-url>`
//!
//! Flow: connect to the source DB and read all users + todos → run the
//! migration against the target DB to create tables → insert row by row →
//! print a summary.
//! Idempotent: rows with an existing id in the target DB are skipped, safe
//! to run repeatedly.
//!
//! Type conversion notes: UUIDs and timestamps (RFC3339) are converted
//! automatically at the driver layer by sea-orm's with-uuid / with-chrono
//! features (sqlite TEXT ↔ pg UUID/TIMESTAMPTZ); the entity layer passes
//! values through directly.

use sea_orm::{ActiveModelTrait, Database, EntityTrait, Set};
use xa_todo_entity as entity;
use xa_todo_migration::{Migrator, MigratorTrait};

/// Runs the migration
pub async fn run(from: &str, to: &str) -> anyhow::Result<()> {
    log::info!("Connecting to source DB: {from}");
    let src = Database::connect(from).await?;
    log::info!("Connecting to target DB: {to}");
    let dst = Database::connect(to).await?;

    // Run migrations against the target DB to create tables (idempotent;
    // already-applied ones are skipped)
    Migrator::up(&dst, None).await?;
    log::info!("Target DB migration complete");

    // ---- users: write users first so the todos foreign key references are
    // satisfiable ----
    let users = entity::user::Entity::find().all(&src).await?;
    let mut users_ok = 0usize;
    let mut users_skip = 0usize;
    for u in users {
        // Idempotent: skip when the target already has the same id
        let exists = entity::user::Entity::find_by_id(u.id).one(&dst).await?;
        if exists.is_some() {
            users_skip += 1;
            continue;
        }
        entity::user::ActiveModel {
            id: Set(u.id),
            name: Set(u.name),
            apikey: Set(u.apikey),
            role: Set(u.role),
            created_at: Set(u.created_at),
        }
        .insert(&dst)
        .await?;
        users_ok += 1;
    }

    // ---- todos ----
    let todos = entity::todo::Entity::find().all(&src).await?;
    let mut todos_ok = 0usize;
    let mut todos_skip = 0usize;
    for t in todos {
        let exists = entity::todo::Entity::find_by_id(t.id).one(&dst).await?;
        if exists.is_some() {
            todos_skip += 1;
            continue;
        }
        entity::todo::ActiveModel {
            id: Set(t.id),
            user_id: Set(t.user_id),
            title: Set(t.title),
            notes: Set(t.notes),
            completed: Set(t.completed),
            due_at: Set(t.due_at),
            created_at: Set(t.created_at),
            updated_at: Set(t.updated_at),
        }
        .insert(&dst)
        .await?;
        todos_ok += 1;
    }

    log::info!(
        "Migration complete: users inserted {users_ok} skipped {users_skip}; todos inserted {todos_ok} skipped {todos_skip}"
    );
    Ok(())
}
