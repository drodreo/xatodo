//! Application shared state

use std::sync::Arc;

use sea_orm::DatabaseConnection;

/// The data-change push handle: after create/update/delete the core fetches
/// the latest full list and hands the serialized JSON over; what the wiring
/// does with it is form-specific —
///
/// - component forms (standalone; connect's events bridge): the integration
///   crate adapts this into the slsdk event plane (one update_data_model
///   message, path=/todos) — every /events subscriber receives it live;
/// - daemon (and any wiring without consumers): a silent drop — the push is
///   best-effort fire-and-forget with no backpressure and no replay, so
///   dropping it is the same observable behaviour as the event plane's
///   no-subscriber drop.
///
/// Cheap to clone (an Arc under the hood).
pub type TodosPush = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

/// axum shared state: database connection pool + data-change push handle
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    /// Push handle invoked by the service layer after create/update/delete.
    /// In event-plane forms it shares the same notifier instance the /events
    /// endpoint serves; push semantics converge at a single point
    /// (service.rs), while the delivery shape is the caller's wiring.
    pub todos_push: TodosPush,
}

impl AppState {
    /// Database-backed state with a silent-drop push (the daemon shape: the
    /// data plane serves REST/MCP and nobody consumes pushes).
    pub fn new(db: DatabaseConnection) -> Self {
        Self::with_push(db, Arc::new(|_| {}))
    }

    /// Database-backed state with an explicit push wiring (the component
    /// shape: the integration crate adapts the slsdk event plane).
    pub fn with_push(db: DatabaseConnection, todos_push: TodosPush) -> Self {
        Self { db, todos_push }
    }
}
