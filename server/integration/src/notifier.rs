//! slsdk event-plane adapter for the core's push handle.
//!
//! The core hands over the latest full todos list as JSON; this adapter
//! turns every push into one `update_data_model(COMPONENT_ID, path=/todos)`
//! event-plane message — the same shape the monolith pushed in-place before
//! the split, so the shell bridge and surfaces keep their wire semantics.

use std::sync::Arc;

use slsdk_rs::{A2uiNotifier, ServerToClientMessage};
use xa_todo_core::state::TodosPush;

use crate::COMPONENT_ID;

/// Adapts an [`A2uiNotifier`] into the core's push handle. The returned
/// closure shares the notifier instance that serve_with_events mounts on
/// /events — pass the same notifier to serve so pushes reach subscribers.
pub fn sl_push_handle(notifier: &A2uiNotifier) -> TodosPush {
    let notifier = notifier.clone();
    Arc::new(move |value| {
        notifier.push(ServerToClientMessage::update_data_model(
            COMPONENT_ID,
            Some("/todos".to_string()),
            value,
        ))
    })
}
