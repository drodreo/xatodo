//! MCP tool definitions and dispatch
//!
//! The four tools share the same permissions as REST (same auth middleware,
//! same user isolation) and the same service layer, guaranteeing consistent
//! REST/MCP behavior.
//!
//! Error semantics contract:
//! - Business errors (missing/invalid args, missing resource, execution
//!   failure) → MCP isError result (tool_error)
//! - Unknown tool name → JSON-RPC error (INVALID_PARAMS)

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use uuid::Uuid;
use xa_todo_entity as entity;

use crate::service::{self, TodoPatch};
use crate::state::AppState;

pub const TOOL_TODO_CREATE: &str = "todo_create";
pub const TOOL_TODO_LIST: &str = "todo_list";
pub const TOOL_TODO_UPDATE: &str = "todo_update";
pub const TOOL_TODO_DELETE: &str = "todo_delete";

/// Tool call error: scenarios that must be converted into a JSON-RPC error
#[derive(Debug)]
pub enum ToolCallError {
    /// Unknown tool name
    UnknownTool(String),
}

/// Tool definitions returned by tools/list (JSON Schema)
pub fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": TOOL_TODO_CREATE,
                "description": "Create a todo",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Todo title (required)" },
                        "notes": { "type": "string", "description": "Notes (optional)" },
                        "due_at": { "type": "string", "description": "Due time, RFC3339 format (optional), e.g. 2026-09-21T18:00:00Z" }
                    },
                    "required": ["title"]
                }
            },
            {
                "name": TOOL_TODO_LIST,
                "description": "List the current user's todos",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "completed": { "type": "boolean", "description": "Filter by completion status (optional)" },
                        "limit": { "type": "number", "description": "Max number of results (optional)" }
                    }
                }
            },
            {
                "name": TOOL_TODO_UPDATE,
                "description": "Update a todo (only the provided fields are modified)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "todo_id": { "type": "string", "description": "Todo UUID (required)" },
                        "title": { "type": "string", "description": "New title (optional)" },
                        "notes": { "type": "string", "description": "New notes (optional)" },
                        "completed": { "type": "boolean", "description": "Completion status (optional)" },
                        "due_at": { "type": "string", "description": "Due time, RFC3339 format (optional)" }
                    },
                    "required": ["todo_id"]
                }
            },
            {
                "name": TOOL_TODO_DELETE,
                "description": "Delete a todo",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "todo_id": { "type": "string", "description": "Todo UUID (required)" }
                    },
                    "required": ["todo_id"]
                }
            }
        ]
    })
}

/// Dispatch tools/call.
/// Ok(Value) = tool execution result (carrying isError semantics);
/// Err = must be converted into a JSON-RPC error.
pub async fn call_tool(
    state: &AppState,
    user: &entity::user::Model,
    name: &str,
    arguments: &Value,
) -> Result<Value, ToolCallError> {
    match name {
        TOOL_TODO_CREATE => todo_create(state, user, arguments).await,
        TOOL_TODO_LIST => todo_list(state, user, arguments).await,
        TOOL_TODO_UPDATE => todo_update(state, user, arguments).await,
        TOOL_TODO_DELETE => todo_delete(state, user, arguments).await,
        other => Err(ToolCallError::UnknownTool(other.to_string())),
    }
}

// ---------- Tool implementations ----------

/// todo_create(title, notes?, due_at?)
async fn todo_create(
    state: &AppState,
    user: &entity::user::Model,
    args: &Value,
) -> Result<Value, ToolCallError> {
    let Some(title) = str_arg(args, "title") else {
        return Ok(tool_error("Missing required argument: title"));
    };
    if title.trim().is_empty() {
        return Ok(tool_error("title must not be empty"));
    }
    let notes = str_arg(args, "notes").map(str::to_string);
    let due_at = match parse_due_at_arg(args) {
        Ok(d) => d,
        Err(msg) => return Ok(tool_error(msg)),
    };

    match service::create_todo(
        &state.db,
        &state.notifier,
        user.id,
        title.to_string(),
        notes,
        due_at,
    )
    .await
    {
        Ok(t) => Ok(tool_text(format!(
            "Todo created: {} (id={})",
            t.title, t.id
        ))),
        Err(e) => Ok(tool_error(format!("Create failed: {e}"))),
    }
}

/// todo_list(completed?, limit?)
async fn todo_list(
    state: &AppState,
    user: &entity::user::Model,
    args: &Value,
) -> Result<Value, ToolCallError> {
    let completed = args.get("completed").and_then(Value::as_bool);
    let limit = args.get("limit").and_then(Value::as_u64);

    let todos = match service::list_todos(&state.db, user.id, completed, limit, None).await {
        Ok(t) => t,
        Err(e) => return Ok(tool_error(format!("Query failed: {e}"))),
    };
    if todos.is_empty() {
        return Ok(tool_text("No todos".to_string()));
    }
    // Plain-text list output (includes id, for update/delete references)
    let lines: Vec<String> = todos
        .iter()
        .map(|t| {
            let due = t
                .due_at
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| "-".to_string());
            format!(
                "[{}] {} | due {} | id={}",
                if t.completed { "x" } else { " " },
                t.title,
                due,
                t.id
            )
        })
        .collect();
    Ok(tool_text(lines.join("\n")))
}

/// todo_update(todo_id, title?, notes?, completed?, due_at?)
async fn todo_update(
    state: &AppState,
    user: &entity::user::Model,
    args: &Value,
) -> Result<Value, ToolCallError> {
    let Some(id) = parse_todo_id_arg(args) else {
        return Ok(tool_error("Missing required argument: todo_id (UUID)"));
    };
    let patch = TodoPatch {
        title: str_arg(args, "title").map(str::to_string),
        notes: str_arg(args, "notes").map(str::to_string),
        completed: args.get("completed").and_then(Value::as_bool),
        due_at: match parse_due_at_arg(args) {
            Ok(d) => d,
            Err(msg) => return Ok(tool_error(msg)),
        },
    };
    if patch.title.is_none()
        && patch.notes.is_none()
        && patch.completed.is_none()
        && patch.due_at.is_none()
    {
        return Ok(tool_error(
            "No fields to update provided (title/notes/completed/due_at)",
        ));
    }

    match service::update_todo(&state.db, &state.notifier, user.id, id, patch).await {
        Ok(Some(_)) => Ok(tool_text(format!("Todo updated: {id}"))),
        Ok(None) => Ok(tool_error(format!("Todo not found: {id}"))),
        Err(e) => Ok(tool_error(format!("Update failed: {e}"))),
    }
}

/// todo_delete(todo_id)
async fn todo_delete(
    state: &AppState,
    user: &entity::user::Model,
    args: &Value,
) -> Result<Value, ToolCallError> {
    let Some(id) = parse_todo_id_arg(args) else {
        return Ok(tool_error("Missing required argument: todo_id (UUID)"));
    };
    match service::delete_todo(&state.db, &state.notifier, user.id, id).await {
        Ok(true) => Ok(tool_text(format!("Todo deleted: {id}"))),
        Ok(false) => Ok(tool_error(format!("Todo not found: {id}"))),
        Err(e) => Ok(tool_error(format!("Delete failed: {e}"))),
    }
}

// ---------- Arg/result helpers ----------

/// Reads a string argument
fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Parses the todo_id argument into a UUID (invalid format is treated as
/// not provided; the required-field validation reports the error)
fn parse_todo_id_arg(args: &Value) -> Option<Uuid> {
    str_arg(args, "todo_id").and_then(|s| Uuid::parse_str(s).ok())
}

/// Parses the due_at argument (RFC3339 string / null / absent → None)
fn parse_due_at_arg(args: &Value) -> Result<Option<DateTime<Utc>>, String> {
    match args.get("due_at") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => DateTime::parse_from_rfc3339(s)
            .map(|d| Some(d.with_timezone(&Utc)))
            .map_err(|e| format!("due_at is not a valid RFC3339 timestamp: {e}")),
        Some(_) => Err("due_at must be an RFC3339 string".to_string()),
    }
}

/// Tool success result (MCP content shape)
fn tool_text(text: String) -> Value {
    json!({ "content": [ { "type": "text", "text": text } ], "isError": false })
}

/// Tool business-error result (MCP isError semantics)
fn tool_error(text: impl Into<String>) -> Value {
    json!({ "content": [ { "type": "text", "text": text.into() } ], "isError": true })
}
