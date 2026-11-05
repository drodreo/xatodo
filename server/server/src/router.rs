//! Route assembly: /api/v1 REST + /mcp MCP
//!
//! Business routes are decoupled from auth: `business_routes` contains only
//! business endpoints, and auth is applied by the caller per mode —
//! standalone mode ([`build`]) applies Bearer apikey auth; component mode
//! (component::build_component_router) applies default owner injection (the
//! shell gateway has already validated the token).

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;

use crate::api;
use crate::auth;
use crate::mcp;
use crate::state::AppState;

/// Business routes (REST + MCP, no auth middleware applied)
pub fn business_routes(state: AppState) -> Router {
    // REST: /api/v1
    let api_router = Router::new()
        .route("/api/v1/me", get(api::me))
        .route(
            "/api/v1/todos",
            get(api::todos::list_todos).post(api::todos::create_todo),
        )
        .route(
            // Note: axum 0.8 path-parameter syntax is {id} (:id in 0.7 and
            // earlier)
            "/api/v1/todos/{id}",
            get(api::todos::get_todo)
                .patch(api::todos::update_todo)
                .delete(api::todos::delete_todo),
        );

    // MCP streamable HTTP: /mcp
    // The MVP shape is POST-only with a single response; GET is not
    // registered, so access automatically returns 405
    let mcp_router = Router::new().route("/mcp", post(mcp::handle_post));

    Router::new()
        .merge(api_router)
        .merge(mcp_router)
        .with_state(state)
}

/// Builds the standalone-mode application router: business routes + Bearer
/// apikey auth
///
/// route_layer only takes effect on registered routes (unmatched paths
/// return 404 rather than 401)
pub fn build(state: AppState) -> Router {
    business_routes(state.clone())
        .route_layer(middleware::from_fn_with_state(state, auth::auth_middleware))
}
