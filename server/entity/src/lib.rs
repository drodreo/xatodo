//! xa-todo-entity: sea-orm entity definitions
//!
//! Table structure:
//! - `user`: the users table, multi-user + apikey auth
//! - `todo`: the todos table, the todo list (FK → users.id)
//!
//! Primary keys are UUID across the project (TEXT in sqlite / UUID in
//! postgres, sea-orm handles the conversion), and times are DateTimeUtc
//! (chrono, serialized by serde as RFC3339 strings).

pub mod todo;
pub mod user;
