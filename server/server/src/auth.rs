//! Bearer apikey auth middleware
//!
//! Auth flow: Authorization: Bearer <apikey> → look up the users table →
//! on hit, the current user entity is injected into request extensions and
//! downstream handlers extract it via Extension;
//! miss / malformed header → 401; database failure → 500.
//!
//! REST (/api/v1) and MCP (/mcp) share this middleware (applied uniformly at
//! the router layer), so the permission surface is identical.
//!
//! In component mode (SL_ENDPOINT injected, launched by the shell) this
//! middleware is not mounted: token auth is handled by slsdk serve, and
//! request attribution is provided by the owner-injection middleware in
//! component.rs using the same extensions shape.

use axum::extract::{Request, State};
use axum::http::{header::AUTHORIZATION, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use xa_todo_entity as entity;

use crate::state::AppState;

/// Auth middleware
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Parse the Bearer token
    let Some(apikey) = extract_bearer(req.headers().get(AUTHORIZATION)) else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    // Look up the users table for the apikey
    let found = entity::user::Entity::find()
        .filter(entity::user::Column::Apikey.eq(apikey))
        .one(&state.db)
        .await;

    match found {
        Ok(Some(user)) => {
            // Hit: inject the current user and let the request through
            req.extensions_mut().insert(user);
            Ok(next.run(req).await)
        }
        Ok(None) => Err(StatusCode::UNAUTHORIZED),
        Err(e) => {
            log::error!("Auth database query failed: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Extracts the Bearer token from the Authorization header
fn extract_bearer(value: Option<&HeaderValue>) -> Option<String> {
    let header = value?.to_str().ok()?;
    let token = header.strip_prefix("Bearer ")?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}
