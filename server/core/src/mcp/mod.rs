//! MCP streamable HTTP endpoint (POST /mcp)
//!
//! MVP shape: hand-written JSON-RPC 2.0 over HTTP, POST one request one
//! response (application/json), without pulling in the rmcp dependency;
//! notifications (no id, e.g. notifications/initialized) return 202 with no
//! body right after being handled.
//!
//! Supported methods: initialize / notifications/initialized / tools/list /
//! tools/call / ping
//!
//! Auth: shares the same Bearer apikey middleware as REST (applied uniformly
//! at the router layer); the current user is extracted from request
//! extensions, and tool data has exactly the same permissions as REST,
//! isolated per user.

pub mod rpc;
pub mod tools;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use serde_json::{json, Value};
use xa_todo_entity as entity;

use crate::state::AppState;
use rpc::{
    RpcErrorResponse, RpcRequest, RpcResponse, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND,
    PARSE_ERROR,
};

/// POST /mcp
pub async fn handle_post(
    State(state): State<AppState>,
    Extension(user): Extension<entity::user::Model>,
    payload: Result<Json<Value>, JsonRejection>,
) -> Response {
    // JSON parse failure → JSON-RPC parse error
    let Json(value) = match payload {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(RpcErrorResponse::new(
                    Value::Null,
                    PARSE_ERROR,
                    format!("JSON parse failed: {e}"),
                )),
            )
                .into_response()
        }
    };

    // One request one response (streamable HTTP MVP does not support batch)
    let req: RpcRequest = match serde_json::from_value(value) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(RpcErrorResponse::new(
                    Value::Null,
                    INVALID_REQUEST,
                    format!("Invalid JSON-RPC request: {e}"),
                )),
            )
                .into_response()
        }
    };

    // Notification: no id, the client expects no response (e.g. the
    // notifications/initialized ack)
    if req.is_notification() {
        return StatusCode::ACCEPTED.into_response();
    }

    // Light jsonrpc version validation
    if req.jsonrpc != "2.0" {
        let id = req.id.clone().unwrap_or(Value::Null);
        return Json(RpcErrorResponse::new(
            id,
            INVALID_REQUEST,
            "jsonrpc must be \"2.0\"",
        ))
        .into_response();
    }

    let id = req.id.clone().unwrap_or(Value::Null);
    let result = match req.method.as_str() {
        // initialize: returns protocol version / capabilities / server info
        "initialize" => initialize_result(),
        // ping: empty result
        "ping" => json!({}),
        // tools/list: the tool inventory (JSON Schema)
        "tools/list" => tools::tool_definitions(),
        // tools/call: tool call dispatch
        "tools/call" => match handle_tools_call(&state, &user, &req.params).await {
            Ok(v) => v,
            Err((code, msg)) => return Json(RpcErrorResponse::new(id, code, msg)).into_response(),
        },
        other => {
            return Json(RpcErrorResponse::new(
                id,
                METHOD_NOT_FOUND,
                format!("Unknown method: {other}"),
            ))
            .into_response()
        }
    };

    Json(RpcResponse::new(id, result)).into_response()
}

/// initialize response payload
fn initialize_result() -> Value {
    json!({
        "protocolVersion": "2025-03-26",
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "xatodo-server",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

/// RPC-layer error: (error code, message)
type RpcErrorPair = (i64, String);

/// Handles tools/call: parses params.name / params.arguments then dispatches
/// to the tool layer
async fn handle_tools_call(
    state: &AppState,
    user: &entity::user::Model,
    params: &Value,
) -> Result<Value, RpcErrorPair> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err((INVALID_PARAMS, "Missing params.name".to_string()));
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match tools::call_tool(state, user, name, &arguments).await {
        Ok(result) => Ok(result),
        Err(tools::ToolCallError::UnknownTool(n)) => {
            Err((INVALID_PARAMS, format!("Unknown tool: {n}")))
        }
    }
}
