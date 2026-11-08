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
}
