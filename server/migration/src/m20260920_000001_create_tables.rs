//! Initial migration: creates the users + todos tables
//!
//! Indexes:
//! - users.apikey unique index (idx_users_apikey)
//! - users.name column-level unique constraint (unique_key, keeps usernames
//!   duplicate-free)
//! - todos.user_id plain index (idx_todos_user_id, speeds up per-user
//!   queries / isolation filters)

use sea_orm_migration::prelude::*;

/// Initial table-creation migration
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ---- users table ----
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    // Primary key UUID, non-autoincrement (TEXT in sqlite /
                    // UUID in pg)
                    .col(ColumnDef::new(Users::Id).uuid().primary_key())
                    .col(ColumnDef::new(Users::Name).string().not_null().unique_key())
                    .col(ColumnDef::new(Users::Apikey).string().not_null())
                    .col(ColumnDef::new(Users::Role).string().not_null())
                    .col(
                        ColumnDef::new(Users::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Explicit unique index on users.apikey
        manager
            .create_index(
                Index::create()
                    .name("idx_users_apikey")
                    .table(Users::Table)
                    .col(Users::Apikey)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // ---- todos table ----
        manager
            .create_table(
                Table::create()
                    .table(Todos::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Todos::Id).uuid().primary_key())
                    .col(ColumnDef::new(Todos::UserId).uuid().not_null())
                    .col(ColumnDef::new(Todos::Title).string().not_null())
                    .col(ColumnDef::new(Todos::Notes).string().null())
                    .col(ColumnDef::new(Todos::Completed).boolean().not_null())
                    .col(
                        ColumnDef::new(Todos::DueAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Todos::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Todos::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // Foreign key: todos.user_id → users.id with cascade
                    // delete (deleting a user removes their todos)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_todos_user_id")
                            .from(Todos::Table, Todos::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Plain index on todos.user_id
        manager
            .create_index(
                Index::create()
                    .name("idx_todos_user_id")
                    .table(Todos::Table)
                    .col(Todos::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    /// Rollback: drop the child table before the parent table (foreign-key
    /// constraint ordering)
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Todos::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await?;
        Ok(())
    }
}

/// users table identifier
#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Name,
    Apikey,
    Role,
    CreatedAt,
}

/// todos table identifier
#[derive(DeriveIden)]
enum Todos {
    Table,
    Id,
    UserId,
    Title,
    Notes,
    Completed,
    DueAt,
    CreatedAt,
    UpdatedAt,
}
