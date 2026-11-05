//! todos entity: the todo list, owned by a user

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// todos table model
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "todos")]
pub struct Model {
    /// Primary key: UUID, non-autoincrement
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Owning user (FK → users.id), the data-isolation boundary
    pub user_id: Uuid,
    /// Title
    pub title: String,
    /// Notes, nullable
    pub notes: Option<String>,
    /// Completion status
    pub completed: bool,
    /// Due time, nullable
    pub due_at: Option<DateTimeUtc>,
    /// Creation time
    pub created_at: DateTimeUtc,
    /// Last update time (refreshed on PATCH)
    pub updated_at: DateTimeUtc,
}

/// Relation: every todo belongs to one user
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UserId",
        to = "super::user::Column::Id"
    )]
    User,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
