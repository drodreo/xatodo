//! xa-todo-connect: the daemon connector form.
//!
//! A pure forwarder with zero local persistence (no database, no migrations,
//! no users sync): it fronts a running daemon for the superlive shell —
//! - REST: any /api/v1/{*rest} request is forwarded to
//!   `{backend.endpoint}{path?query}` with `Authorization: Bearer
//!   backend.apikey` (method / body / query pass through; status, body and
//!   content-type pass back);
//! - MCP: POST /mcp is forwarded the same way;
//! - /events bridge: a background task subscribes to the daemon's
//!   `{backend.endpoint}/events` (NDJSON stream, apikey-authenticated) and
//!   feeds every line into the slsdk event plane via A2uiNotifier::push —
//!   surfaces refresh live; on stream loss (daemon restart, network blip)
//!   it reconnects with capped exponential backoff. A daemon without an
//!   event-plane endpoint (404) simply keeps the bridge retrying at the
//!   capped interval: harmless no-op, REST/MCP forwarding is unaffected.
//!
//! manifest: self-reported via make_manifest_with_config() (id=xatodo — the
//! shell matches by id; the same constant every other form reports). The
//! config declaration is compiled in from the packaging chain's own
//! connect-config.json, so the process-side declaration matches the shipped
//! manifest.json the shell config page renders from.
//! config: [backend] is required; listen / [users] / database_url are parsed
//! (rigid schema) but not consumed. GET/PUT /config is mounted (manifest
//! declaration + ConfigIntegration, write-back to config.toml in the working
//! directory — the same file Config::load reads on the next start).

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use futures_util::StreamExt;
use log::{info, warn};
use serde_json::json;
use slsdk_rs::{A2uiNotifier, ServerToClientMessage};
use xa_todo_core::config::{BackendCfg, Config};
use xa_todo_integration as integration;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(e) = run().await {
        log::error!("Connector startup failed: {e:#}");
        std::process::exit(1);
    }
}

/// Connector startup: load config → require backend → spawn the events
/// bridge → assemble the forward router → slsdk serve.
/// Blocks until graceful shutdown (SIGTERM is handled by serve's built-in
/// listener).
async fn run() -> anyhow::Result<()> {
    let cfg = Config::load().await?;
    let backend = cfg.require_backend()?.clone();
    info!(
        "Backend: endpoint={}, system proxy {}",
        backend.endpoint,
        if backend.use_system_proxy {
            "on"
        } else {
            "off (direct)"
        }
    );

    let notifier = A2uiNotifier::new();
    // Events bridge: daemon event plane → slsdk notifier (→ shell surfaces)
    tokio::spawn(events_bridge(backend.clone(), notifier.clone()));

    let app = forward_router(backend);
    // Config declaration + integration both present → slsdk mounts the
    // standard GET/PUT /config endpoint (the shell's config save path). The
    // declaration is the same file the packaging chain weaves into the
    // shipped manifest.json; the file path matches Config::load's resolution
    // (config.toml in the component's working directory = install directory
    // under the shell), so a write-back is picked up by the next start.
    // All three declared fields are restart_required (no on_change hook).
    let manifest = integration::make_manifest_with_config();
    let config = slsdk_rs::ConfigIntegration {
        path: std::path::PathBuf::from("config.toml"),
        on_change: None,
    };
    let serve_info = slsdk_rs::serve_with_events(app, &manifest, Some(config), notifier).await?;
    info!("Connector service started: {}", serve_info.endpoint);
    serve_info.wait().await?;
    Ok(())
}

/// Shared forwarder context: one pooled HTTP client + the backend
/// declaration (cheap to clone — Arc under the hood).
#[derive(Clone)]
struct ForwardCtx {
    client: reqwest::Client,
    backend: Arc<BackendCfg>,
}

/// The connector's outbound client. Proxy behavior is config-driven via
/// [backend].use_system_proxy: default false disables proxies entirely
/// (`ClientBuilder::no_proxy`) — the backend endpoint is a user-configured
/// service address with direct-reach semantics, and reqwest otherwise
/// inherits ambient proxy settings (http_proxy/https_proxy env), silently
/// rerouting intranet forwarding through a proxy that cannot reach the
/// target. true lets the client follow ambient proxy settings, for
/// networks whose path to the daemon requires a proxy.
fn outbound_client(use_system_proxy: bool) -> reqwest::Client {
    let mut builder = reqwest::Client::builder();
    if !use_system_proxy {
        builder = builder.no_proxy();
    }
    builder
        .build()
        .expect("client build cannot fail with only no_proxy set")
}

/// The forward router: everything under /api/v1 plus /mcp, all methods.
fn forward_router(backend: BackendCfg) -> Router {
    let ctx = ForwardCtx {
        client: outbound_client(backend.use_system_proxy),
        backend: Arc::new(backend),
    };
    Router::new()
        .route("/api/v1/{*rest}", axum::routing::any(forward))
        .route("/mcp", axum::routing::post(forward))
        .with_state(ctx)
}

/// Transparent forward: method / path / query / body / content-type pass
/// through; the Authorization header is replaced with the backend apikey;
/// status / body / content-type pass back.
async fn forward(
    State(ctx): State<ForwardCtx>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Path + query as received (shell forwarding semantics: the connector
    // sees the complete path space)
    let path_and_query = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| uri.path().to_string());
    let url = format!(
        "{}{}",
        ctx.backend.endpoint.trim_end_matches('/'),
        path_and_query
    );

    // reqwest re-exports the same http crate types axum uses, so the method
    // passes through unchanged (consumed by request(); cloned for the error
    // log below)
    let mut req = ctx
        .client
        .request(method.clone(), &url)
        .bearer_auth(&ctx.backend.apikey);
    if let Some(ct) = headers.get(axum::http::header::CONTENT_TYPE) {
        req = req.header(axum::http::header::CONTENT_TYPE, ct);
    }
    if !body.is_empty() {
        req = req.body(body.to_vec());
    }

    match req.send().await {
        Ok(resp) => translate_response(&method, resp).await,
        Err(e) => {
            // Full source chain: the top-level reqwest display alone ("error
            // sending request for url") hides whether the death was a refused
            // connection, a timeout, or a proxy failure — the chain names it.
            let mut chain = e.to_string();
            let mut src = std::error::Error::source(&e);
            while let Some(s) = src {
                chain.push_str(&format!(": {s}"));
                src = s.source();
            }
            warn!("Forward {method} {url} failed: {chain}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("backend unreachable: {chain}") })),
            )
                .into_response()
        }
    }
}

/// Passes the daemon's response back: status + body + content-type (every
/// other transport header is the connector's own business).
async fn translate_response(method: &Method, resp: reqwest::Response) -> Response {
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).cloned();
    // The URL is needed in the read-failure log below, and bytes() consumes
    // the response — take a cheap clone (Arc under the hood) first.
    let url = resp.url().clone();
    match resp.bytes().await {
        Ok(bytes) => {
            let mut builder = Response::builder().status(status);
            if let Some(ct) = content_type {
                builder = builder.header(axum::http::header::CONTENT_TYPE, ct);
            }
            builder
                .body(axum::body::Body::from(bytes))
                .unwrap_or_else(|e| {
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({ "error": format!("response assembly failed: {e}") })),
                    )
                        .into_response()
                })
        }
        Err(e) => {
            warn!("Backend response read failed {method} {url}: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("backend response read failed: {e}") })),
            )
                .into_response()
        }
    }
}

/// Bridges the daemon's event plane into the slsdk notifier: subscribes to
/// `{endpoint}/events` and pushes every NDJSON line as a
/// [`ServerToClientMessage`]. On stream loss it reconnects with exponential
/// backoff (2s doubling, capped at 30s; reset on a clean stream end).
async fn events_bridge(backend: BackendCfg, notifier: A2uiNotifier) {
    let client = outbound_client(backend.use_system_proxy);
    let mut backoff = Duration::from_secs(2);
    loop {
        let url = format!("{}/events", backend.endpoint.trim_end_matches('/'));
        match subscribe_stream(&client, &url, &backend.apikey, &notifier).await {
            Ok(()) => {
                // Stream ended cleanly (daemon shutting down): reset backoff
                info!("Events bridge: stream ended, will reconnect");
                backoff = Duration::from_secs(2);
            }
            Err(e) => {
                warn!("Events bridge: {url} failed: {e}; retrying in {backoff:?}");
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

/// One subscription lifetime: reads the NDJSON stream until it ends.
/// Per the B-plane wire contract: one [`ServerToClientMessage`] per
/// `\n`-terminated line, no SSE framing, no heartbeat frames; a trailing
/// partial line stays buffered until its terminator arrives.
async fn subscribe_stream(
    client: &reqwest::Client,
    url: &str,
    apikey: &str,
    notifier: &A2uiNotifier,
) -> anyhow::Result<()> {
    let resp = client
        .get(url)
        .bearer_auth(apikey)
        .send()
        .await
        .context("events subscribe request failed")?
        .error_for_status()
        .context("events subscribe rejected by daemon")?;

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("events stream read failed")?;
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]);
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<ServerToClientMessage>(line) {
                Ok(msg) => notifier.push(msg),
                Err(e) => warn!("Events bridge: skipping unparsable line: {e}"),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use slsdk_rs::Endpoint;

    /// Regression anchor for the "component config endpoint returned 404"
    /// incident: the connector must mount the standard GET/PUT /config
    /// endpoint. slsdk only mounts it when BOTH sides are present — the
    /// manifest declares a config section (make_manifest_with_config, the
    /// packaging chain's own declaration) AND a ConfigIntegration is handed
    /// to serve_with_events. Full save path under one serving instance:
    /// GET 200 → PUT 200 → write-back landed in config.toml → read-back
    /// returns the new value.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn config_endpoint_mounts_and_persists_write_back() {
        // The declaration must survive the compile-time round trip: same
        // field count the packaging chain ships (endpoint/apikey/
        // use_system_proxy).
        let manifest = integration::make_manifest_with_config();
        let config = manifest
            .config
            .as_ref()
            .expect("connect manifest declares a config section");
        assert_eq!(
            config.fields.len(),
            3,
            "declaration must match scripts/connect-config.json"
        );
        assert_eq!(config.fields[0].key, "backend.endpoint");

        // Serve the same wiring run() uses, with the write-back target in a
        // temp dir (the production path convention is config.toml in the
        // working directory; tests may pass an absolute path). No
        // SL_ENDPOINT/SL_TOKEN in the test process → random-port debug
        // serving without auth, unless the environment injects a token
        // (honored per-request, never mutated — tests run in parallel).
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            // apikey must be non-empty: the endpoint's merged-state
            // validation treats empty strings on required fields as missing
            // (first-install semantics), so an empty-seed PUT would 400 by
            // design.
            "[backend]\nendpoint = \"http://placeholder\"\napikey = \"test-key\"\n",
        )
        .unwrap();

        let serve_info = slsdk_rs::serve_with_events(
            axum::Router::new(),
            &manifest,
            Some(slsdk_rs::ConfigIntegration {
                path: config_path.clone(),
                on_change: None,
            }),
            A2uiNotifier::new(),
        )
        .await
        .expect("serve assembly succeeds");

        let addr = match serve_info.endpoint {
            Endpoint::Tcp { addr } => addr,
            other => panic!("expected tcp debug endpoint, got {other}"),
        };
        let base = format!("http://{addr}");
        let token = std::env::var(slsdk_rs::ENV_TOKEN).ok();
        let client = reqwest::Client::new();

        // 1. GET /config: the mounted endpoint answers 200 — this exact
        //    request was the 404 the shell surfaced ("component config
        //    endpoint returned 404").
        let mut get = client.get(format!("{base}/config"));
        if let Some(t) = &token {
            get = get.bearer_auth(t);
        }
        let resp = get.send().await.unwrap();
        let status = resp.status();
        assert_eq!(status, reqwest::StatusCode::OK, "GET /config must not 404");
        let merged: serde_json::Value =
            serde_json::from_slice(&resp.bytes().await.unwrap()).unwrap();
        assert_eq!(
            merged["values"]["backend.endpoint"], "http://placeholder",
            "GET returns the file's current value (flat dotted keys)"
        );

        // 2. PUT /config with a legal value: 200 + the write-back reaches
        //    config.toml (preserving unrecognized sections). The body is
        //    serialized by hand: the workspace reqwest has no `json`
        //    feature (forwarding passes raw bytes) and tests don't widen it.
        let mut put = client
            .put(format!("{base}/config"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(
                serde_json::to_vec(&serde_json::json!({
                    "backend.endpoint": "http://192.168.2.40"
                }))
                .unwrap(),
            );
        if let Some(t) = &token {
            put = put.bearer_auth(t);
        }
        let resp = put.send().await.unwrap();
        let status = resp.status();
        assert_eq!(status, reqwest::StatusCode::OK, "PUT /config must succeed");
        let written = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            written.contains("http://192.168.2.40"),
            "write-back must land in config.toml, got: {written}"
        );
        assert!(
            written.contains("apikey"),
            "unrecognized sections are preserved on write-back, got: {written}"
        );

        // 3. Read-back closes the loop: the endpoint now returns the new
        //    value (same shape the shell refreshes from after a save).
        let mut get = client.get(format!("{base}/config"));
        if let Some(t) = &token {
            get = get.bearer_auth(t);
        }
        let resp = get.send().await.unwrap();
        let merged: serde_json::Value =
            serde_json::from_slice(&resp.bytes().await.unwrap()).unwrap();
        assert_eq!(merged["values"]["backend.endpoint"], "http://192.168.2.40");

        serve_info.shutdown();
    }
    use tokio::sync::Mutex;

    /// One request as seen by the fake daemon (the forwarder's inbound face).
    #[derive(Debug, Clone)]
    struct Recorded {
        method: String,
        path_and_query: String,
        authorization: Option<String>,
        content_type: Option<String>,
        body: String,
    }

    type RecordLog = Arc<Mutex<Vec<Recorded>>>;

    /// Fake daemon state: the request log + the canned response.
    type FakeState = (RecordLog, StatusCode, Arc<str>);

    /// Captures every request aspect the forward contract cares about and
    /// answers with the canned response (status + application/json body).
    async fn capture(
        State((log, status, payload)): State<FakeState>,
        method: Method,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        log.lock().await.push(Recorded {
            method: method.to_string(),
            path_and_query: uri
                .path_and_query()
                .map(|pq| pq.as_str().to_string())
                .unwrap_or_else(|| uri.path().to_string()),
            authorization: headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
            content_type: headers
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
            body: String::from_utf8_lossy(&body).to_string(),
        });
        (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            payload.as_ref().to_owned(),
        )
            .into_response()
    }

    /// Serves a router on an ephemeral localhost port; returns the address.
    /// The serve task is dropped when the test runtime shuts down.
    async fn serve(router: Router) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral bind");
        let addr = listener.local_addr().expect("local_addr");
        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test server should stay up");
        });
        addr
    }

    fn fake_daemon(log: RecordLog, status: StatusCode, payload: &str) -> Router {
        Router::new()
            .route("/{*rest}", axum::routing::any(capture))
            .with_state((log, status, Arc::from(payload)))
    }

    fn backend_cfg(endpoint: String) -> BackendCfg {
        BackendCfg {
            endpoint,
            apikey: API_KEY.to_string(),
            use_system_proxy: false,
        }
    }

    const API_KEY: &str = "test-api-key";

    // ===== REST/MCP forwarding =====

    #[tokio::test(flavor = "multi_thread")]
    async fn forwards_method_path_query_and_bearer_auth_to_backend() {
        let log: RecordLog = Arc::new(Mutex::new(Vec::new()));
        let daemon = serve(fake_daemon(log.clone(), StatusCode::OK, r#"{"ok":true}"#)).await;
        let connector = serve(forward_router(backend_cfg(format!("http://{daemon}")))).await;

        let client = outbound_client(false);
        let resp = client
            .get(format!("http://{connector}/api/v1/todo/list?done=false"))
            .header(axum::http::header::AUTHORIZATION, "Bearer shell-token")
            .send()
            .await
            .expect("connector request should succeed");

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json"),
            "the daemon's content-type must pass back"
        );
        assert_eq!(resp.text().await.expect("body"), r#"{"ok":true}"#);

        let log = log.lock().await;
        assert_eq!(log.len(), 1, "exactly one backend hit");
        let rec = &log[0];
        assert_eq!(rec.method, "GET");
        assert_eq!(rec.path_and_query, "/api/v1/todo/list?done=false");
        assert_eq!(
            rec.authorization.as_deref(),
            Some("Bearer test-api-key"),
            "the backend must see the configured apikey, not the shell token"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn forwards_post_body_and_passes_non_default_status_back() {
        let log: RecordLog = Arc::new(Mutex::new(Vec::new()));
        let body = r#"{"title":"write tests"}"#;
        let daemon = serve(fake_daemon(log.clone(), StatusCode::CREATED, r#"{"id":7}"#)).await;
        let connector = serve(forward_router(backend_cfg(format!("http://{daemon}")))).await;

        let client = outbound_client(false);
        let resp = client
            .post(format!("http://{connector}/api/v1/todo"))
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(body.to_owned())
            .send()
            .await
            .expect("connector request should succeed");

        assert_eq!(resp.status(), StatusCode::CREATED);
        assert_eq!(resp.text().await.expect("body"), r#"{"id":7}"#);

        let log = log.lock().await;
        assert_eq!(log.len(), 1);
        let rec = &log[0];
        assert_eq!(rec.method, "POST");
        assert_eq!(rec.path_and_query, "/api/v1/todo");
        assert_eq!(rec.body, body, "the request body must pass through");
        assert_eq!(rec.content_type.as_deref(), Some("application/json"));
        assert_eq!(rec.authorization.as_deref(), Some("Bearer test-api-key"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn backend_error_status_passes_through_unchanged() {
        let log: RecordLog = Arc::new(Mutex::new(Vec::new()));
        let daemon = serve(fake_daemon(
            log.clone(),
            StatusCode::NOT_FOUND,
            r#"{"error":"no todo"}"#,
        ))
        .await;
        let connector = serve(forward_router(backend_cfg(format!("http://{daemon}")))).await;

        let client = outbound_client(false);
        let resp = client
            .get(format!("http://{connector}/api/v1/todo/999"))
            .send()
            .await
            .expect("connector request should succeed");

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.text().await.expect("body"), r#"{"error":"no todo"}"#);
        assert_eq!(log.lock().await.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unreachable_backend_yields_502_with_error_body() {
        // A bound-then-dropped listener = a closed port: connect refused.
        let dead_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral bind");
        let dead = dead_listener.local_addr().expect("local_addr");
        drop(dead_listener);

        let connector = serve(forward_router(backend_cfg(format!("http://{dead}")))).await;
        let client = outbound_client(false);
        let resp = client
            .get(format!("http://{connector}/api/v1/todo/list"))
            .send()
            .await
            .expect("the connector itself must answer");

        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
        let body = resp.text().await.expect("body");
        assert!(
            body.contains("backend unreachable"),
            "error body should name the failure, got: {body}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn mcp_post_is_forwarded_the_same_way() {
        let log: RecordLog = Arc::new(Mutex::new(Vec::new()));
        let body = r#"{"jsonrpc":"2.0","id":1}"#;
        let daemon = serve(fake_daemon(
            log.clone(),
            StatusCode::OK,
            r#"{"jsonrpc":"2.0"}"#,
        ))
        .await;
        let connector = serve(forward_router(backend_cfg(format!("http://{daemon}")))).await;

        let client = outbound_client(false);
        let resp = client
            .post(format!("http://{connector}/mcp"))
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(body.to_owned())
            .send()
            .await
            .expect("connector request should succeed");

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.text().await.expect("body"), r#"{"jsonrpc":"2.0"}"#);

        let log = log.lock().await;
        assert_eq!(log.len(), 1);
        let rec = &log[0];
        assert_eq!(rec.method, "POST");
        assert_eq!(rec.path_and_query, "/mcp");
        assert_eq!(rec.body, body);
        assert_eq!(rec.authorization.as_deref(), Some("Bearer test-api-key"));
    }

    // ===== use_system_proxy =====

    /// Proxy behavior is env-driven and captured once at client build time,
    /// so both sides run inside this one test: env mutation is process
    /// global, but every other test builds `no_proxy` clients (which never
    /// read env — that is the whole point of the default), and the rest of
    /// the workspace's tests run in separate test binaries. The env window
    /// lives only between the set/remove pair below.
    #[tokio::test(flavor = "multi_thread")]
    async fn use_system_proxy_off_bypasses_env_proxy_and_on_follows_it() {
        // A dead proxy: a bound-then-dropped listener = a closed port, so a
        // proxied request dies at the connect stage (connection refused).
        let dead_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral bind");
        let dead = dead_listener.local_addr().expect("local_addr");
        drop(dead_listener);

        // A real backend on a live port.
        let log: RecordLog = Arc::new(Mutex::new(Vec::new()));
        let backend = serve(fake_daemon(log, StatusCode::OK, "{}")).await;
        let target = format!("http://{backend}/api/v1/todo/list");

        std::env::set_var("http_proxy", format!("http://{dead}"));
        // Keep an ambient NO_PROXY (e.g. a developer's local proxy config)
        // from bypassing the dead proxy on the `on` side.
        std::env::set_var("NO_PROXY", "");

        // Both clients are built while the env window is open: the `on`
        // client captures the ambient proxy in ClientBuilder::build.
        let direct = outbound_client(false);
        let proxied = outbound_client(true);

        // off (the serde default): no_proxy() disables proxies entirely —
        // the ambient http_proxy must be bypassed and the request must land
        // on the real backend.
        let resp = direct
            .get(&target)
            .send()
            .await
            .expect("proxy-off request should reach the backend directly");
        assert_eq!(resp.status(), StatusCode::OK, "direct request must arrive");

        std::env::remove_var("http_proxy");
        std::env::remove_var("NO_PROXY");

        // on: the client follows the captured ambient proxy, which is dead —
        // the request must fail at the connect stage.
        let err = proxied
            .get(&target)
            .send()
            .await
            .expect_err("proxied request must fail on the dead proxy");
        assert!(
            err.is_connect(),
            "expected a connect-stage failure via the dead proxy, got: {err}"
        );
    }

    // ===== events bridge =====

    #[tokio::test(flavor = "multi_thread")]
    async fn events_bridge_relays_ndjson_lines_into_the_notifier() {
        // Consumer side: the slsdk /events plane fed by the notifier (the
        // same public router the shell bridges against).
        let notifier = A2uiNotifier::new();
        let consumer = serve(slsdk_rs::events::events_router(notifier.clone())).await;

        // Producer side: a fake daemon whose /events pushes one NDJSON line
        // then ends the stream (the relay lifetime completes on its own).
        let msg = ServerToClientMessage::update_data_model("s1", None, serde_json::json!({}));
        let line = serde_json::to_string(&msg).expect("serialize");
        let daemon = serve(Router::new().route(
            "/events",
            axum::routing::get(move |headers: HeaderMap| async move {
                if headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    != Some("Bearer test-api-key")
                {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                (
                    StatusCode::OK,
                    [(axum::http::header::CONTENT_TYPE, "application/x-ndjson")],
                    format!("{line}\n"),
                )
                    .into_response()
            }),
        ))
        .await;

        // Open the consumer stream first and wait for its broadcast receiver
        // to register: the plane has no replay, so a push with zero
        // subscribers is silently dropped.
        let consumer_resp = outbound_client(false)
            .get(format!("http://{consumer}/events"))
            .send()
            .await
            .expect("consumer stream should open");
        assert_eq!(consumer_resp.status(), StatusCode::OK);
        tokio::time::timeout(Duration::from_secs(5), async {
            while notifier.subscriber_count() == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("consumer subscriber should register in time");

        // Relay: one subscribe_stream lifetime over the fake daemon.
        let client = outbound_client(false);
        let relay = {
            let notifier = notifier.clone();
            let url = format!("http://{daemon}/events");
            tokio::spawn(async move { subscribe_stream(&client, &url, API_KEY, &notifier).await })
        };

        // The relayed message must arrive on the consumer stream with its
        // semantics intact.
        let mut stream = consumer_resp.bytes_stream();
        let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("relay should produce a line in time")
            .expect("consumer stream should yield a chunk")
            .expect("consumer chunk read should succeed");
        let got: ServerToClientMessage =
            serde_json::from_str(String::from_utf8_lossy(&chunk).trim())
                .expect("relayed line should parse");
        assert_eq!(got, msg, "relayed message semantics");
        assert!(
            relay.await.expect("relay task").is_ok(),
            "one clean stream end"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn events_bridge_reports_rejection_on_missing_event_plane() {
        // A daemon without an event-plane endpoint: the 404 must surface as
        // the "rejected by daemon" error (the backoff-retried shape).
        let log: RecordLog = Arc::new(Mutex::new(Vec::new()));
        let daemon = serve(fake_daemon(log, StatusCode::NOT_FOUND, "{}")).await;

        let client = outbound_client(false);
        let err = subscribe_stream(
            &client,
            &format!("http://{daemon}/events"),
            API_KEY,
            &A2uiNotifier::new(),
        )
        .await
        .expect_err("a 404 event plane must be an error");
        assert!(err.to_string().contains("rejected by daemon"), "got: {err}");
    }
}
