//! REST handlers: the /api/v1/todos family
//!
//! All require auth (unified middleware at the router layer); the current
//! user is extracted from request extensions, and data operations are
//! delegated to the service layer (isolated per user).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xa_todo_entity as entity;

use crate::error::{ApiError, ApiResult};
use crate::service;
use crate::state::AppState;

/// API response shape: entity fields flattened + a self-describing resource
/// path.
/// `self` carries the full relative path (e.g. `api/v1/todos/{id}`), so
/// surface row actions reach it directly via data binding — clients never
/// assemble the path themselves.
#[derive(Debug, Serialize)]
pub struct TodoResource {
    #[serde(flatten)]
    pub todo: entity::todo::Model,
    #[serde(rename = "self")]
    pub self_path: String,
}

impl From<entity::todo::Model> for TodoResource {
    fn from(todo: entity::todo::Model) -> Self {
        let self_path = format!("api/v1/todos/{}", todo.id);
        Self { todo, self_path }
    }
}

/// GET /api/v1/todos query params: completed / limit / offset are all
/// optional
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub completed: Option<bool>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

/// GET /api/v1/todos: list (filter/pagination optional)
pub async fn list_todos(
    State(state): State<AppState>,
    Extension(user): Extension<entity::user::Model>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<TodoResource>>> {
    let todos = service::list_todos(&state.db, user.id, q.completed, q.limit, q.offset).await?;
    Ok(Json(todos.into_iter().map(TodoResource::from).collect()))
}

/// POST /api/v1/todos request body
#[derive(Debug, Deserialize)]
pub struct CreateTodoReq {
    pub title: String,
    pub notes: Option<String>,
    /// RFC3339 string, nullable
    pub due_at: Option<DateTime<Utc>>,
}

/// POST /api/v1/todos: create, returns 201
pub async fn create_todo(
    State(state): State<AppState>,
    Extension(user): Extension<entity::user::Model>,
    Json(req): Json<CreateTodoReq>,
) -> ApiResult<(StatusCode, Json<TodoResource>)> {
    if req.title.trim().is_empty() {
        return Err(ApiError::BadRequest("title must not be empty".into()));
    }
    let todo = service::create_todo(
        &state.db,
        &state.todos_push,
        user.id,
        req.title,
        req.notes,
        req.due_at,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(TodoResource::from(todo))))
}

/// GET /api/v1/todos/{id}: detail
pub async fn get_todo(
    State(state): State<AppState>,
    Extension(user): Extension<entity::user::Model>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TodoResource>> {
    let todo = service::get_todo(&state.db, user.id, id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(TodoResource::from(todo)))
}

/// PATCH /api/v1/todos/{id} request body: only provided fields are modified
#[derive(Debug, Deserialize)]
pub struct UpdateTodoReq {
    pub title: Option<String>,
    /// Note: in the MVP, passing null for notes is equivalent to omitting it
    /// (no modification); clearing via null is not supported
    pub notes: Option<String>,
    pub completed: Option<bool>,
    pub due_at: Option<DateTime<Utc>>,
}

/// PATCH /api/v1/todos/{id}: partial update (updated_at refreshed
/// automatically)
pub async fn update_todo(
    State(state): State<AppState>,
    Extension(user): Extension<entity::user::Model>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTodoReq>,
) -> ApiResult<Json<TodoResource>> {
    let patch = service::TodoPatch {
        title: req.title,
        notes: req.notes,
        completed: req.completed,
        due_at: req.due_at,
    };
    let todo = service::update_todo(&state.db, &state.todos_push, user.id, id, patch)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(TodoResource::from(todo)))
}

/// DELETE /api/v1/todos/{id}: delete, returns 204 on success
pub async fn delete_todo(
    State(state): State<AppState>,
    Extension(user): Extension<entity::user::Model>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    if service::delete_todo(&state.db, &state.todos_push, user.id, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}
