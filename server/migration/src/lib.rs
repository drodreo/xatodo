//! xa-todo-migration: database migrations
//!
//! This crate is independent of xa-todo-entity (no reverse dependency on
//! business entities); xa-todo-server invokes it in the startup flow and the
//! migrate subcommand.
//!
//! ┌─ Project migration discipline (all future migrations must follow) ────
//! │ Every future DROP / ALTER column or DROP table operation must first
//! │ check with manager.has_column() / has_table() (operate only when the
//! │ column/table exists), so a missing column/table in the target DB does
//! │ not abort the run with an error.
//! │ Pattern: check first → change → return.
//! └────────────────────────────────────────────────────────────

pub use sea_orm_migration::prelude::*;

mod m20260920_000001_create_tables;

/// Migrator: runs all migrations in order
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260920_000001_create_tables::Migration)]
    }
}
