//! Application shared state

use sea_orm::DatabaseConnection;
use slsdk_rs::A2uiNotifier;

/// axum shared state: database connection pool + event-plane push handle
/// (cheap to Clone, the internals are handles)
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    /// A2UI event-plane push handle: the service layer pushes the latest
    /// list through it after create/update/delete.
    /// In component mode it shares the same instance as the /events endpoint
    /// mounted by serve_with_events;
    /// in standalone mode there are no consumers, and pushes are silently
    /// dropped per the event-plane contract (equivalent to a no-op).
    pub notifier: A2uiNotifier,
}

impl AppState {
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            notifier: A2uiNotifier::new(),
        }
    }
}
