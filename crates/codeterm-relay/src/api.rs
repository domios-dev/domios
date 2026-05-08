//! REST API endpoints for codeterm-relay.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;
use axum::{
    extract::{Path, Query, State, Extension},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post, delete},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use crate::config::Config;
use crate::routes::RouteTable;
use crate::webhook::{render_template, TemplateContext, verify_webhook};
use crate::error::RelayError;
use codeterm_mesh_client::types::{
    MeshIdentity, PairedPeer, RelayCommand, RelayResponse,
};

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub routes: Arc<Mutex<RouteTable>>,
    pub identity: Arc<MeshIdentity>,
    // TODO: Add mesh state (paired peers, signed token cache)
    // pub mesh: MeshState,
}

impl AppState {
    pub fn new(config: Config, routes: RouteTable, identity: MeshIdentity) -> Self {
        Self {
            config,
            routes: Arc::new(Mutex::new(routes)),
            identity: Arc::new(identity),
        }
    }
}

/// Create the Axum router.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Health and identity
        .route("/api/health", get(health_handler))
        .route("/api/identify", get(identify_handler))
        // Peers
        .route("/api/peers", get(list_peers_handler))
        .route("/api/peers/{id}/panes", get(peer_panes_handler))
        // Send text to pane
        .route("/api/send", post(send_handler))
        // Pane output
        .route("/api/panes/{id}/output", get(pane_output_handler))
        .route("/api/panes/{id}/output/stream", get(pane_output_stream_handler))
        // Routes management
        .route("/api/routes", get(list_routes_handler))
        .route("/api/routes/reload", post(reload_routes_handler))
        // Webhooks
        .route("/webhook/{route_id}", post(webhook_handler))
        // SPA
        .route("/", get(spa_handler))
        .fallback(get(spa_fallback_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ── Health & Identity ───────────────────────────────────────────────

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn identify_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "instance_id": state.identity.instance_id,
        "instance_name": state.identity.instance_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "capabilities": ["pane_read", "pane_send_text", "notify"],
    }))
}

// ── Bearer Auth ─────────────────────────────────────────────────────

fn check_bearer_auth(state: &AppState, headers: &HeaderMap) -> Result<(), RelayError> {
    let token = state.config.relay.bearer_token.as_ref()
        .ok_or_else(|| RelayError::AuthFailed("No bearer token configured".to_string()))?;

    let auth_header = headers.get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| RelayError::AuthFailed("Missing Authorization header".to_string()))?;

    let token = token.trim_start_matches("Bearer ");

    if auth_header == format!("Bearer {}", token) {
        Ok(())
    } else {
        Err(RelayError::AuthFailed("Invalid bearer token".to_string()))
    }
}

// ── Peers ────────────────────────────────────────────────────────────

/// List paired full peers.
async fn list_peers_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, RelayError> {
    check_bearer_auth(&state, &headers)?;

    // TODO: Query actual mesh state
    // For now, return empty list until mesh is integrated
    Json(serde_json::json!({
        "peers": []
    })).pipe(Ok)
}

/// List panes on a specific peer.
async fn peer_panes_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(peer_id): Path<String>,
) -> Result<Json<serde_json::Value>, RelayError> {
    check_bearer_auth(&state, &headers)?;

    // TODO: Query peer via mesh
    // For now, return empty
    Json(serde_json::json!({
        "peer_id": peer_id,
        "panes": []
    })).pipe(Ok)
}

// ── Send Text ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SendRequest {
    /// Target peer instance_id or instance_name
    pub peer: String,
    /// Target pane id or name
    pub pane: String,
    /// Text to send
    pub text: String,
    /// Whether to press Enter (default: true)
    #[serde(default = "default_true")]
    pub enter: bool,
}

fn default_true() -> bool { true }

/// Send text to a pane on a paired peer.
async fn send_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SendRequest>,
) -> Result<Json<serde_json::Value>, RelayError> {
    check_bearer_auth(&state, &headers)?;

    // TODO: Look up peer in mesh state, get signed token, make API call
    // For now, return a placeholder response
    tracing::info!(
        "Send request: peer={}, pane={}, text={}, enter={}",
        body.peer, body.pane, body.text, body.enter
    );

    Json(serde_json::json!({
        "ok": true,
        "peer": body.peer,
        "pane": body.pane,
        "sent": body.text,
        "enter": body.enter,
    })).pipe(Ok)
}

// ── Pane Output ─────────────────────────────────────────────────────

/// Get output from a pane.
async fn pane_output_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, RelayError> {
    check_bearer_auth(&state, &headers)?;

    let offset = params.get("offset")
        .and_then(|v| v.parse().ok());

    // TODO: Get output from pane via mesh
    Json(serde_json::json!({
        "pane_id": pane_id,
        "output": "",
        "offset": offset.unwrap_or(0),
    })).pipe(Ok)
}

/// SSE stream of pane output.
async fn pane_output_stream_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pane_id): Path<String>,
) -> Result<impl axum::response::IntoResponse, RelayError> {
    check_bearer_auth(&state, &headers)?;

    // TODO: Stream output from pane via mesh
    let stream = async_stream::stream! {
        yield Ok::<_, std::convert::Infallible>(axum::response::sse::Event::default()
            .data(serde_json::json!({"pane_id": pane_id, "output": "", "offset": 0}).to_string()));
    };

    Ok(axum::response::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default()))
}

// ── Routes Management ────────────────────────────────────────────────

async fn list_routes_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let routes = state.routes.lock();
    let route_list: Vec<_> = routes.routes.values().map(|r| {
        serde_json::json!({
            "name": r.name,
            "path": r.r#match.path,
            "target_peer": r.target.peer,
            "target_pane": r.target.pane,
        })
    }).collect();

    Json(serde_json::json!({
        "routes": route_list
    }))
}

async fn reload_routes_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let routes = state.routes.lock();
    Json(serde_json::json!({
        "ok": true,
        "route_count": routes.routes.len(),
    }))
}

// ── Webhooks ─────────────────────────────────────────────────────────

/// Handle incoming webhooks.
async fn webhook_handler(
    State(state): State<AppState>,
    Path(route_id): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<serde_json::Value>, RelayError> {
    let route = {
        let routes = state.routes.lock();
        routes.routes.get(&route_id).cloned()
    };

    let route = route.ok_or_else(|| RelayError::RouteNotFound(route_id.clone()))?;

    // Convert headers to HashMap
    let header_map: HashMap<String, String> = headers.iter()
        .filter_map(|(k, v)| {
            v.to_str().ok().map(|s| (k.to_string(), s.to_string()))
        })
        .collect();

    // Parse JSON body
    let json_body: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or(serde_json::Value::Null);

    // Check route match
    let path = format!("/webhook/{}", route_id);
    if route.matches(&path, &header_map, &json_body).is_none() {
        return Err(RelayError::InvalidRequest("Request does not match route conditions".to_string()));
    }

    // Verify webhook signature if auth is configured
    if let Some(ref auth) = route.auth {
        let signature = header_map.get("X-Hub-Signature-256")
            .or_else(|| header_map.get("X-Stripe-Signature"))
            .cloned();
        let timestamp = header_map.get("X-Stripe-Timestamp").cloned();

        // Get secret (from env var)
        let secret = auth.secret.replace("${", "").replace("}", "");

        verify_webhook(auth, signature.as_deref(), timestamp.as_deref(), body.as_bytes(), &secret)
            .map_err(|e| RelayError::WebhookVerificationFailed(e.to_string()))?;
    }

    // Render template
    let ctx = TemplateContext::new(json_body, &header_map, path, &HashMap::new());
    let message = render_template(&route.template, &ctx);

    // TODO: Send to mesh peer
    tracing::info!(
        "Webhook matched: route={}, peer={}, pane={}, message={}",
        route.name, route.target.peer, route.target.pane, message
    );

    Ok(Json(serde_json::json!({
        "ok": true,
        "route": route.name,
        "peer": route.target.peer,
        "pane": route.target.pane,
        "message": message,
    })))
}

// ── SPA ─────────────────────────────────────────────────────────────

async fn spa_handler() -> impl IntoResponse {
    // TODO: Serve embedded SPA
    // For now, return a simple placeholder
    (
        StatusCode::OK,
        [("content-type", "text/html")],
        r#"<!DOCTYPE html>
<html>
<head>
    <title>CodeTerm Relay</title>
    <style>
        body { font-family: system-ui; max-width: 800px; margin: 50px auto; padding: 20px; }
        h1 { color: #333; }
        .info { background: #f5f5f5; padding: 20px; border-radius: 8px; }
        .api { margin-top: 20px; }
        code { background: #eee; padding: 2px 6px; border-radius: 4px; }
    </style>
</head>
<body>
    <h1>CodeTerm Relay</h1>
    <div class="info">
        <p><strong>Status:</strong> Running</p>
        <p><strong>Version:</strong> 0.1.0</p>
        <p><strong>Instance ID:</strong> Check /api/identify</p>
    </div>
    <div class="api">
        <h2>API Endpoints</h2>
        <ul>
            <li><code>GET /api/health</code> - Health check</li>
            <li><code>GET /api/identify</code> - Relay identity</li>
            <li><code>GET /api/peers</code> - List paired peers</li>
            <li><code>POST /api/send</code> - Send text to pane</li>
            <li><code>POST /webhook/{route_id}</code> - Webhook endpoint</li>
        </ul>
    </div>
</body>
</html>"#,
    )
}

async fn spa_fallback_handler() -> impl IntoResponse {
    spa_handler().into_response()
}

// ── Helpers ──────────────────────────────────────────────────────────

trait IntoJsonResponse {
    fn pipe<T: Serialize>(self, Ok_: impl FnOnce(Self) -> Result<Json<T>, RelayError>) -> Result<Json<T>, RelayError>;
}

impl<T> IntoJsonResponse for T {
    fn pipe<U: Serialize>(self, Ok_: impl FnOnce(T) -> Result<Json<U>, RelayError>) -> Result<Json<U>, RelayError> {
        Ok_(self)
    }
}