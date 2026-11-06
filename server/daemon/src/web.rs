//! Static web hosting for the daemon form (optional SPA frontend).
//!
//! Driven by `[server].web_root` in config.toml: when set, the daemon serves
//! the directory over the same listener as the API — registered API routes
//! win, and every unmatched request goes to the static service (a file in
//! the directory; a miss falls back to index.html, the SPA entry). When the
//! field is absent or the directory does not exist, the router is returned
//! unchanged (API-only behavior; a bad directory logs a warning instead of
//! failing startup — the daemon's API service must not go down over a
//! frontend misconfiguration).
//!
//! Daemon-only wiring: standalone (the shell hosts the surface bundle) and
//! connect (a pure forwarder) never touch this module. The static service is
//! mounted via `fallback_service`, which is outside the Bearer apikey
//! `route_layer` — static assets are served without authentication, while
//! the API keeps its auth wall.

use std::path::PathBuf;

use axum::Router;
use log::{info, warn};
use tower_http::services::{ServeDir, ServeFile};

/// Attaches SPA static hosting to the daemon router when `web_root` is
/// configured; returns the router unchanged when the field is absent or the
/// directory does not exist.
pub fn attach(app: Router, web_root: Option<&str>) -> Router {
    let Some(root) = web_root.map(PathBuf::from) else {
        return app;
    };
    if !root.is_dir() {
        warn!(
            "web_root '{}' does not exist or is not a directory; static web hosting disabled",
            root.display()
        );
        return app;
    }
    // SPA convention: directory listing is not exposed; any path that does
    // not resolve to a file (including the root itself through
    // append_index_html_on_directories) is answered with index.html.
    let index = root.join("index.html");
    let static_service = ServeDir::new(root)
        .append_index_html_on_directories(true)
        .not_found_service(ServeFile::new(index));
    info!("Static web hosting attached (SPA fallback to index.html)");
    app.fallback_service(static_service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use std::fs;
    use std::path::PathBuf;
    use tower::ServiceExt; // oneshot

    /// Builds a throwaway static directory: index.html + assets/app.js
    fn fixture_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("xatodo-web-test-{tag}-{}", std::process::id()));
        fs::create_dir_all(dir.join("assets")).unwrap();
        fs::write(dir.join("index.html"), "<html>spa-index</html>").unwrap();
        fs::write(dir.join("assets/app.js"), "console.log('app');").unwrap();
        dir
    }

    fn cleanup(dir: &PathBuf) {
        let _ = fs::remove_dir_all(dir);
    }

    async fn get(app: Router, uri: &str) -> (StatusCode, String) {
        let res = app
            .oneshot(HttpRequest::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn static_file_is_served() {
        let dir = fixture_dir("hit");
        let app = attach(Router::new(), Some(dir.to_str().unwrap()));

        let (status, body) = get(app, "/assets/app.js").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "console.log('app');");

        cleanup(&dir);
    }

    #[tokio::test]
    async fn spa_fallback_serves_index() {
        let dir = fixture_dir("spa");
        let app = attach(Router::new(), Some(dir.to_str().unwrap()));

        // Unknown frontend route: the not-found fallback serves index.html.
        // The tower-http SPA convention marks fallback responses 404
        // (not_found_service wraps the fallback in SetStatus(404)); the
        // browser still renders the body.
        let (status, body) = get(app.clone(), "/some/frontend/route").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, "<html>spa-index</html>");

        // The root itself: real index.html with 200
        // (append_index_html_on_directories)
        let (status, body) = get(app, "/").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "<html>spa-index</html>");

        cleanup(&dir);
    }

    #[tokio::test]
    async fn no_web_root_mounts_no_static_service() {
        let app = attach(Router::new(), None);

        let (status, _) = get(app, "/whatever").await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "without web_root the router must stay API-only (no fallback service)"
        );
    }

    #[tokio::test]
    async fn missing_directory_skips_hosting() {
        let missing =
            std::env::temp_dir().join(format!("xatodo-web-missing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&missing);
        let app = attach(Router::new(), Some(missing.to_str().unwrap()));

        let (status, _) = get(app, "/").await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a nonexistent web_root must be skipped (warn), not fail or host"
        );
    }
}
