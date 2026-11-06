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
//! manifest: self-reported via make_manifest() (id=xatodo — the shell
//! matches by id; the same constant every other form reports).
//! config: [backend] is required; listen / [users] / database_url are parsed
//! (rigid schema) but not consumed.

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

    let notifier = A2uiNotifier::new();
    // Events bridge: daemon event plane → slsdk notifier (→ shell surfaces)
    tokio::spawn(events_bridge(backend.clone(), notifier.clone()));

    let app = forward_router(backend);
    let manifest = integration::make_manifest();
    let serve_info = slsdk_rs::serve_with_events(app, &manifest, None, notifier).await?;
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

/// The forward router: everything under /api/v1 plus /mcp, all methods.
fn forward_router(backend: BackendCfg) -> Router {
    let ctx = ForwardCtx {
        client: reqwest::Client::new(),
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
        Ok(resp) => translate_response(resp).await,
        Err(e) => {
            warn!("Forward {} {} failed: {e}", method, uri.path());
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("backend unreachable: {e}") })),
            )
                .into_response()
        }
    }
}

/// Passes the daemon's response back: status + body + content-type (every
/// other transport header is the connector's own business).
async fn translate_response(resp: reqwest::Response) -> Response {
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).cloned();
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
            warn!("Backend response read failed: {e}");
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
    let client = reqwest::Client::new();
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
                warn!("Events bridge: {e}; retrying in {:?}", backoff);
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
