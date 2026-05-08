//! REST API endpoints for codeterm-relay.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use tower_http::cors::CorsLayer;

use crate::config::Config;
use crate::routes::RouteTable;
use crate::webhook::{render_template, TemplateContext, verify_webhook};
use crate::error::RelayError;
use codeterm_mesh_client::types::MeshIdentity;

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub routes: Arc<Mutex<RouteTable>>,
    pub identity: Arc<MeshIdentity>,
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
        // Health and identity (public)
        .route("/api/health", get(health_handler))
        .route("/api/identify", get(identify_handler))
        // Peers (authenticated)
        .route("/api/peers", get(with_auth(list_peers_handler)))
        .route("/api/peers/{id}/panes", get(with_auth(peer_panes_handler)))
        // Send text to pane (authenticated)
        .route("/api/send", post(with_auth(send_handler)))
        // Pane output (authenticated)
        .route("/api/panes/{id}/output", get(with_auth(pane_output_handler)))
        .route("/api/panes/{id}/output/stream", get(with_auth(pane_output_stream_handler)))
        // Routes management (public)
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

// ── Auth Middleware ─────────────────────────────────────────────────

/// Wrapper that enforces bearer token auth.
macro_rules! with_auth {
    ($handler:expr) => {
        |State(state): State<AppState>, headers: HeaderMap, body| {
            check_bearer_auth(&state, &headers).map_err(|e| e.into_response())?;
            $handler(State(state), headers, body)
        }
    };
}

fn check_bearer_auth(state: &AppState, headers: &HeaderMap) -> Result<(), RelayError> {
    let token = state.config.relay.bearer_token.as_ref()
        .ok_or_else(|| RelayError::AuthFailed("No bearer token configured".to_string()))?;

    let auth_header = headers.get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| RelayError::AuthFailed("Missing Authorization header".to_string()))?;

    let expected = format!("Bearer {}", token);
    if auth_header == expected {
        Ok(())
    } else {
        Err(RelayError::AuthFailed("Invalid bearer token".to_string()))
    }
}

// ── Health & Identity ───────────────────────────────────────────────

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn identify_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "instance_id": state.identity.instance_id,
        "instance_name": state.identity.instance_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "capabilities": ["pane_read", "pane_send_text", "notify"],
    }))
}

// ── Peers ────────────────────────────────────────────────────────────

async fn list_peers_handler() -> impl IntoResponse {
    // TODO: Query actual mesh state
    Json(serde_json::json!({ "peers": [] }))
}

async fn peer_panes_handler(Path(peer_id): Path<String>) -> impl IntoResponse {
    // TODO: Query peer via mesh
    Json(serde_json::json!({
        "peer_id": peer_id,
        "panes": []
    }))
}

// ── Send Text ────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct SendRequest {
    pub peer: String,
    pub pane: String,
    pub text: String,
    #[serde(default = "default_true")]
    pub enter: bool,
}

fn default_true() -> bool { true }

async fn send_handler(Json(body): Json<SendRequest>) -> impl IntoResponse {
    tracing::info!(
        "Send request: peer={}, pane={}, text={}, enter={}",
        body.peer, body.pane, body.text, body.enter
    );

    // TODO: Look up peer in mesh state, get signed token, make API call
    Json(serde_json::json!({
        "ok": true,
        "peer": body.peer,
        "pane": body.pane,
        "sent": body.text,
        "enter": body.enter,
    }))
}

// ── Pane Output ─────────────────────────────────────────────────────

async fn pane_output_handler(
    Path(pane_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let offset = params.get("offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // TODO: Get output from pane via mesh
    Json(serde_json::json!({
        "pane_id": pane_id,
        "output": "",
        "offset": offset,
    }))
}

async fn pane_output_stream_handler(
    Path(pane_id): Path<String>,
) -> impl IntoResponse {
    // TODO: Stream output from pane via mesh
    let stream = async_stream::stream! {
        yield Ok::<_, std::convert::Infallible>(
            axum::response::sse::Event::default()
                .data(serde_json::json!({"pane_id": pane_id, "output": "", "offset": 0}).to_string())
        );
    };

    axum::response::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
}

// ── Routes Management ───────────────────────────────────────────────

async fn list_routes_handler(State(state): State<AppState>) -> impl IntoResponse {
    let routes = state.routes.lock();
    let route_list: Vec<_> = routes.routes.values().map(|r| {
        serde_json::json!({
            "name": r.name,
            "path": r.r#match.path,
            "target_peer": r.target.peer,
            "target_pane": r.target.pane,
        })
    }).collect();

    Json(serde_json::json!({ "routes": route_list }))
}

async fn reload_routes_handler(State(state): State<AppState>) -> impl IntoResponse {
    let routes = state.routes.lock();
    Json(serde_json::json!({
        "ok": true,
        "route_count": routes.routes.len(),
    }))
}

// ── Webhooks ─────────────────────────────────────────────────────────

async fn webhook_handler(
    State(state): State<AppState>,
    Path(route_id): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Result<impl IntoResponse, impl IntoResponse> {
    // Get route
    let route = {
        let routes = state.routes.lock();
        routes.routes.get(&route_id).cloned()
    };
    let route = route.ok_or_else(|| RelayError::RouteNotFound(route_id.clone()))?;

    // Convert headers to HashMap
    let header_map: HashMap<String, String> = headers.iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.to_string(), s.to_string())))
        .collect();

    // Parse JSON body
    let json_body = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);

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
        let secret = auth.secret.replace("${", "").replace("}", "");

        verify_webhook(auth, signature.as_deref(), timestamp.as_deref(), body.as_bytes(), &secret)
            .map_err(|e| RelayError::WebhookVerificationFailed(e.to_string()))?;
    }

    // Render template
    let ctx = TemplateContext::new(json_body, &header_map, path, &HashMap::new());
    let message = render_template(&route.template, &ctx);

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

const SPA_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>CodeTerm Relay</title>
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body { font-family: system-ui, -apple-system, sans-serif; max-width: 800px; margin: 0 auto; padding: 20px; background: #f5f5f5; }
        h1 { color: #1a1a1a; margin-bottom: 20px; }
        .card { background: white; border-radius: 8px; padding: 20px; margin-bottom: 16px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }
        .status { display: flex; align-items: center; gap: 8px; }
        .badge { background: #22c55e; color: white; padding: 4px 12px; border-radius: 9999px; font-size: 14px; font-weight: 500; }
        .api-list { list-style: none; }
        .api-list li { padding: 12px 0; border-bottom: 1px solid #eee; display: flex; gap: 12px; }
        .api-list li:last-child { border-bottom: none; }
        code { background: #f0f0f0; padding: 4px 8px; border-radius: 4px; font-size: 14px; }
        .method { color: #22c55e; font-weight: 600; min-width: 40px; }
        .method.post { color: #3b82f6; }
    </style>
</head>
<body>
    <h1>CodeTerm Relay</h1>
    <div class="card">
        <div class="status">
            <span class="badge">Running</span>
            <span>v0.1.0</span>
        </div>
    </div>
    <div class="card">
        <h2>API Endpoints</h2>
        <ul class="api-list">
            <li><span class="method">GET</span> <code>/api/health</code> - Health check</li>
            <li><span class="method">GET</span> <code>/api/identify</code> - Relay identity</li>
            <li><span class="method">GET</span> <code>/api/peers</code> - List paired peers</li>
            <li><span class="method post">POST</span> <code>/api/send</code> - Send text to pane</li>
            <li><span class="method">GET</span> <code>/api/routes</code> - List routes</li>
            <li><span class="method post">POST</span> <code>/webhook/{route_id}</code> - Webhook endpoint</li>
        </ul>
    </div>
</body>
</html>"#;

async fn spa_handler() -> impl IntoResponse {
    (StatusCode::OK, [("content-type", "text/html")], SPA_HTML)
}

async fn spa_fallback_handler() -> impl IntoResponse {
    spa_handler()
}
