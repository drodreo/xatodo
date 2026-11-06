//! Owner attribution for component forms: a single local owner.
//!
//! In component forms all requests are attributed to one users-table row
//! (name="local", UUID primary key): there is no apikey auth inside the
//! component (the shell gateway validates the per-component token, slsdk
//! serve re-validates SL_TOKEN as the first wall), so the owner-injection
//! middleware replaces the core's auth middleware with the same extensions
//! shape — downstream handlers extract the user via Extension, unaware of
//! the form difference.
//! users_sync is skipped in component forms: config.toml's [users.entries]
//! is the authority source for the daemon deployment.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use log::info;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;
use xa_todo_entity as entity;

/// Username of the component-form default owner: all requests are attributed
/// to this user.
pub const LOCAL_OWNER_NAME: &str = "local";

/// Ensures the default owner exists and returns it: looks up by name="local",
/// inserts on miss (UUID primary key).
/// The apikey field (unique, non-null) is filled with a random UUID — in
/// component forms apikey does not participate in auth.
pub async fn ensure_local_owner(db: &DatabaseConnection) -> anyhow::Result<entity::user::Model> {
    let existing = entity::user::Entity::find()
        .filter(entity::user::Column::Name.eq(LOCAL_OWNER_NAME))
        .one(db)
        .await?;
    if let Some(user) = existing {
        return Ok(user);
    }
    let user = entity::user::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(LOCAL_OWNER_NAME.to_string()),
        apikey: Set(Uuid::new_v4().to_string()),
        role: Set(entity::user::UserRole::User),
        created_at: Set(chrono::Utc::now()),
    }
    .insert(db)
    .await?;
    info!("Default owner \"{LOCAL_OWNER_NAME}\" created");
    Ok(user)
}

/// Owner injection middleware: performs no auth, puts the default owner
/// entity into the request extensions.
/// The injection shape is identical to daemon-form auth_middleware —
/// downstream handlers extract the user via Extension, unaware of the form
/// difference.
pub async fn inject_owner(
    State(owner): State<entity::user::Model>,
    mut req: Request,
    next: Next,
) -> Response {
    req.extensions_mut().insert(owner);
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{cleanup, test_db};

    #[tokio::test]
    async fn local_owner_created_once() {
        let (db, path) = test_db("owner").await;
        let first = ensure_local_owner(&db).await.unwrap();
        assert_eq!(first.name, LOCAL_OWNER_NAME);

        // Repeated ensure must reuse the same owner, and the table holds
        // exactly one row
        let second = ensure_local_owner(&db).await.unwrap();
        assert_eq!(
            first.id, second.id,
            "repeated ensure must reuse the same owner"
        );
        let all = entity::user::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 1);
        cleanup(&path);
    }
}
