//! todo CRUD core logic (service layer)
//!
//! Shared by REST handlers and MCP tools so both sides behave identically
//! with identical permissions: every operation filters by user_id and data is
//! isolated per user.

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use uuid::Uuid;
use xa_todo_entity as entity;

use crate::state::TodosPush;

/// todo update payload: None = leave the field unchanged
#[derive(Debug, Default, Clone)]
pub struct TodoPatch {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub completed: Option<bool>,
    pub due_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Unified push point after data changes: fetches the user's latest full
/// list and hands the serialized JSON to the push handle (the wiring turns
/// it into an update_data_model event / a drop). Every successful REST/MCP
/// create/update/delete goes through here — push semantics converge at a
/// single point:
/// - Event-plane forms (standalone / connect bridge): all /events subscribers
///   (shell bridge → live refresh on every surface) receive it live;
/// - Wireings without consumers (daemon): the value is silently dropped —
///   equivalent to a no-op;
/// - Fetch or serialization failures are logged but not propagated: the push
///   is a best-effort notification, and failure is naturally healed by the
///   next change (subscribers also re-pull the full list on reconnect).
async fn notify_todos_changed(todos_push: &TodosPush, db: &DatabaseConnection, user_id: Uuid) {
    let todos = match list_todos(db, user_id, None, None, None).await {
        Ok(todos) => todos,
        Err(e) => {
            log::warn!("todo change push failed (skipped, waiting for next change or reconnect re-pull): {e}");
            return;
        }
    };
    // Serialization failure skips with the same semantics: pushing an empty
    // array would make subscribers see a false "list cleared" state.
    let value = match serde_json::to_value(&todos) {
        Ok(value) => value,
        Err(e) => {
            log::warn!("todo serialization failed (push skipped, waiting for next change or reconnect re-pull): {e}");
            return;
        }
    };
    todos_push(value);
}

/// Lists a user's todos (completed filter and limit/offset pagination are
/// both optional)
pub async fn list_todos(
    db: &DatabaseConnection,
    user_id: Uuid,
    completed: Option<bool>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<entity::todo::Model>, DbErr> {
    let mut select = entity::todo::Entity::find()
        .filter(entity::todo::Column::UserId.eq(user_id))
        .order_by_asc(entity::todo::Column::CreatedAt);
    if let Some(c) = completed {
        select = select.filter(entity::todo::Column::Completed.eq(c));
    }
    if let Some(l) = limit {
        select = select.limit(l);
    }
    if let Some(o) = offset {
        select = select.offset(o);
    }
    select.all(db).await
}

/// Creates a todo (completed starts fixed at false; updated_at equals
/// created_at). Hands the latest list to the push handle on success.
pub async fn create_todo(
    db: &DatabaseConnection,
    todos_push: &TodosPush,
    user_id: Uuid,
    title: String,
    notes: Option<String>,
    due_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<entity::todo::Model, DbErr> {
    let now = Utc::now();
    let todo = entity::todo::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(user_id),
        title: Set(title),
        notes: Set(notes),
        completed: Set(false),
        due_at: Set(due_at),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;
    notify_todos_changed(todos_push, db, user_id).await;
    Ok(todo)
}

/// Fetches a single todo (with user isolation: other users' ids are never
/// found)
pub async fn get_todo(
    db: &DatabaseConnection,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<entity::todo::Model>, DbErr> {
    entity::todo::Entity::find_by_id(id)
        .filter(entity::todo::Column::UserId.eq(user_id))
        .one(db)
        .await
}

/// Updates a todo: only fields present in the patch are modified, and
/// updated_at is refreshed.
/// Returns None when the todo does not exist (or belongs to another user).
/// Hands the latest list to the push handle after a successful update.
pub async fn update_todo(
    db: &DatabaseConnection,
    todos_push: &TodosPush,
    user_id: Uuid,
    id: Uuid,
    patch: TodoPatch,
) -> Result<Option<entity::todo::Model>, DbErr> {
    // Query then modify: doubles as existence check and user isolation
    let Some(existing) = get_todo(db, user_id, id).await? else {
        return Ok(None);
    };
    let mut am: entity::todo::ActiveModel = existing.into();
    if let Some(t) = patch.title {
        am.title = Set(t);
    }
    if let Some(n) = patch.notes {
        am.notes = Set(Some(n));
    }
    if let Some(c) = patch.completed {
        am.completed = Set(c);
    }
    if let Some(d) = patch.due_at {
        am.due_at = Set(Some(d));
    }
    // Whatever changed, updated_at is always refreshed
    am.updated_at = Set(Utc::now());
    let todo = am.update(db).await?;
    notify_todos_changed(todos_push, db, user_id).await;
    Ok(Some(todo))
}

/// Deletes a todo. Returns false when it does not exist (or belongs to
/// another user). Hands the latest list to the push handle after a
/// successful delete.
pub async fn delete_todo(
    db: &DatabaseConnection,
    todos_push: &TodosPush,
    user_id: Uuid,
    id: Uuid,
) -> Result<bool, DbErr> {
    let res = entity::todo::Entity::delete_many()
        .filter(entity::todo::Column::Id.eq(id))
        .filter(entity::todo::Column::UserId.eq(user_id))
        .exec(db)
        .await?;
    let deleted = res.rows_affected > 0;
    if deleted {
        notify_todos_changed(todos_push, db, user_id).await;
    }
    Ok(deleted)
}
