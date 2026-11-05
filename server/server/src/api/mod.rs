//! REST API module

pub mod todos;

use axum::Extension;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use xa_todo_entity as entity;

/// GET /api/v1/me: returns the current authenticated user's info
///
/// Note: the apikey is never echoed back (auth credentials are not exposed
/// through query endpoints); only the fields needed for display are returned
#[derive(Debug, Serialize)]
pub struct MeResp {
    pub id: Uuid,
    pub name: String,
    pub role: entity::user::UserRole,
    pub created_at: DateTime<Utc>,
}

pub async fn me(Extension(user): Extension<entity::user::Model>) -> Json<MeResp> {
    Json(MeResp {
        id: user.id,
        name: user.name,
        role: user.role,
        created_at: user.created_at,
    })
}
