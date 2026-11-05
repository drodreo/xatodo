//! users sync: config.toml's [users.entries] is the apikey authority
//!
//! Synced into the users table at startup:
//! - New name → insert (fresh UUID)
//! - Existing name → update apikey / role by name (id / created_at untouched)

use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;
use xa_todo_entity as entity;

use crate::config::UserEntry;

/// Syncs all user entries from the config
pub async fn sync_users(db: &DatabaseConnection, entries: &[UserEntry]) -> anyhow::Result<()> {
    for entry in entries {
        let existing = entity::user::Entity::find()
            .filter(entity::user::Column::Name.eq(&entry.name))
            .one(db)
            .await?;

        match existing {
            Some(model) => {
                // Existing: update apikey / role by name
                let mut am: entity::user::ActiveModel = model.into();
                am.apikey = Set(entry.apikey.clone());
                am.role = Set(entry.role.clone());
                am.update(db).await?;
                log::info!("users sync: user {} updated", entry.name);
            }
            None => {
                // New: insert
                entity::user::ActiveModel {
                    id: Set(Uuid::new_v4()),
                    name: Set(entry.name.clone()),
                    apikey: Set(entry.apikey.clone()),
                    role: Set(entry.role.clone()),
                    created_at: Set(chrono::Utc::now()),
                }
                .insert(db)
                .await?;
                log::info!("users sync: user {} created", entry.name);
            }
        }
    }
    Ok(())
}
