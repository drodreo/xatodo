//! Unified API error type: propagated by REST handlers via ApiResult and
//! converted uniformly into JSON error responses by IntoResponse

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

/// API error
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Resource not found (or not accessible — under user isolation, other
    /// users' resources are always treated as not found)
    #[error("resource not found")]
    NotFound,
    /// Invalid request parameters
    #[error("bad request: {0}")]
    BadRequest(String),
    /// Database error (details never exposed externally, logged only)
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ApiError::Db(e) => {
                log::error!("Database error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_string(),
                )
            }
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

/// Unified return type for handlers
pub type ApiResult<T> = Result<T, ApiError>;
