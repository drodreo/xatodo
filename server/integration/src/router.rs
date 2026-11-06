//! Component-form router: business routes (REST + MCP) + owner injection
//! middleware (replacing apikey auth).
//! /health is mounted by slsdk serve (SDK-reserved path; business routes
//! must not register the same path).

use axum::Router;
use xa_todo_core::router::business_routes;
use xa_todo_core::state::AppState;
use xa_todo_entity as entity;

use crate::owner;

/// Component-form router: business routes + owner injection
pub fn build_component_router(state: AppState, owner: entity::user::Model) -> Router {
    business_routes(state).route_layer(axum::middleware::from_fn_with_state(
        owner,
        owner::inject_owner,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{cleanup, test_db};
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use tower::ServiceExt; // oneshot

    #[tokio::test]
    async fn component_router_serves_owner_without_apikey() {
        let (db, path) = test_db("router").await;
        let owner = owner::ensure_local_owner(&db).await.unwrap();
        let app = build_component_router(AppState::new(db), owner);

        // Reachable without an Authorization header: apikey auth is bypassed
        // (token validation is handled at the slsdk serve layer)
        let res = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/v1/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let me: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(me["name"], owner::LOCAL_OWNER_NAME);

        // Write path: created todo is attributed to the default owner
        let res = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/v1/todos")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"component mode smoke"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let todo: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(todo["title"], "component mode smoke");

        cleanup(&path);
    }

    #[tokio::test]
    async fn component_router_serves_mcp() {
        let (db, path) = test_db("mcp").await;
        let owner = owner::ensure_local_owner(&db).await.unwrap();
        let app = build_component_router(AppState::new(db), owner);

        // The MCP path works as usual in component mode (owner injection,
        // one request one response)
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        });
        let res = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let rpc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(rpc["jsonrpc"], "2.0");
        assert!(
            rpc["result"]["tools"].is_array(),
            "tools/list should return an array of tools"
        );

        cleanup(&path);
    }
}
