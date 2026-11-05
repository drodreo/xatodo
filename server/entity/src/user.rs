//! users entity: multi-user + apikey auth

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// User role: string enum, stored in the DB as "user" / "assistant"
#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
// serde external representation aligned with string_value: JSON output is
// lowercase "user" / "assistant"
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    #[sea_orm(string_value = "user")]
    User,
    #[sea_orm(string_value = "assistant")]
    Assistant,
}

/// users table model
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    /// Primary key: UUID, non-autoincrement (TEXT in sqlite / UUID in pg)
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Username, unique
    #[sea_orm(unique)]
    pub name: String,
    /// Bearer auth apikey, unique
    #[sea_orm(unique)]
    pub apikey: String,
    /// Role: "user" | "assistant" (stored as a string enum)
    pub role: UserRole,
    /// Creation time
    pub created_at: DateTimeUtc,
}

/// Relation: one user owns many todos
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::todo::Entity")]
    Todo,
}

impl Related<super::todo::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Todo.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
