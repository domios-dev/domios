use std::sync::Arc;
use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tauri::Emitter;
use tower_http::cors::CorsLayer;

use crate::api::events::WebhookRegistration;
use crate::screenshot::ScreenshotState;

#[derive(Clone)]
pub struct AppState {
    pub svc: crate::service::ServiceState,
    pub token: Option<String>,
    #[allow(dead_code)]
    pub screenshot: ScreenshotState,
    pub cron: crate::CronState,
    pub cron_wake: crate::CronWake,
    pub api_port: u16,
    pub update_state: crate::UpdateStateHandle,
    pub config: crate::config::ConfigState,
    pub mesh: crate::MeshStateHandle,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/identify", get(identify))
        .route("/api/workspaces", get(list_workspaces).post(create_workspace))
        .route("/api/workspaces/{id}/focus", post(focus_workspace))
        .route("/api/workspaces/{id}/rename", post(rename_workspace))
        .route("/api/workspaces/{id}/close", post(close_workspace))
        .route("/api/workspaces/{id}/tabs", post(create_tab))
        .route("/api/workspaces/{id}/tabs/{tab_id}/move", post(move_tab))
        .route("/api/panes", get(list_panes))
        .route("/api/panes/{id}/split", post(split_pane))
        .route("/api/panes/{id}/send", post(send_text))
        .route("/api/panes/{id}/focus", post(focus_pane))
        .route("/api/panes/{id}/close", post(close_pane))
        .route("/api/panes/{id}/rename", post(rename_pane))
        .route("/api/panes/{id}/output", get(get_pane_output))
        .route("/api/panes/{id}/output/stream", get(stream_pane_output))
        .route("/api/panes/{id}/screen", get(get_pane_screen))
        .route("/api/panes/{id}/exec", post(exec_in_pane))
        .route("/api/panes/{id}/resize", post(resize_pane))
        .route("/api/browser/open", post(open_browser))
        .route("/api/serve-file", get(serve_file))
        .route("/api/notifications", get(list_notifications).post(create_notification))
        .route("/api/notifications/clear", post(clear_notifications))
        .route("/api/webhooks", get(list_webhooks).post(register_webhook))
        .route("/api/webhooks/{id}", axum::routing::delete(unregister_webhook))
        .route("/api/screenshot", get(take_screenshot))
        .route("/api/network", get(network_info))
        .route("/api/themes", get(list_themes_api).post(save_theme_api))
        .route("/api/themes/{id}", axum::routing::delete(delete_theme_api))
        .route("/api/themes/apply", post(apply_theme_api))
        // Setup endpoints — skills, rules, hooks
        .route("/api/setup/skills", get(list_skills_api))
        .route("/api/setup/skills/{id}/install", post(install_skill_api))
        .route("/api/setup/skills/{id}/uninstall", post(uninstall_skill_api))
        .route("/api/setup/rules", get(check_rules_api))
        .route("/api/setup/rules/install", post(install_rules_api))
        .route("/api/setup/hooks", get(check_hooks_api))
        .route("/api/setup/hooks/install", post(install_hooks_api))
        .route("/api/setup/hooks/uninstall", post(uninstall_hooks_api))
        // Agent hierarchy
        .route("/api/panes/{id}/role", post(set_pane_role).delete(clear_pane_role))
        .route("/api/panes/{id}/status", get(get_pane_status))
        .route("/api/panes/{id}/children", get(get_pane_children))
        .route("/api/panes/{id}/report", post(submit_report))
        .route("/api/panes/{id}/reports", get(get_reports))
        .route("/api/panes/{id}/can-spawn", post(set_can_spawn))
        .route("/api/panes/{id}/agent-type", post(set_agent_type))
        .route("/api/panes/{id}/prompt/respond", post(respond_prompt))
        .route("/api/panes/{id}/prompt/clear", post(clear_prompt))
        .route("/api/panes/{id}/hook/permission", post(hook_permission))
        .route("/api/panes/{id}/hook/stop", post(hook_stop))
        .route("/api/agents", get(list_agents))
        .route("/api/agents/hierarchy", get(get_hierarchy))
        .route("/api/agents/spawn", post(spawn_agent))
        .route("/api/batch", post(batch_commands))
        // Cron jobs
        .route("/api/crons", get(cron_list).post(cron_create))
        .route("/api/crons/{id}", get(cron_get).put(cron_update).delete(cron_delete))
        .route("/api/crons/{id}/enable", post(cron_enable))
        .route("/api/crons/{id}/disable", post(cron_disable))
        .route("/api/crons/{id}/trigger", post(cron_trigger))
        .route("/api/crons/{id}/history", get(cron_history))
        // Update
        .route("/api/config", get(get_config_api).patch(patch_config_api))
        .route("/api/update/check", get(check_update))
        .route("/api/update/install", post(install_update))
        .route("/api/bridge/send", post(bridge_send_api))
        // Mesh networking
        .route("/api/mesh/ping", get(mesh_ping))
        .route("/api/mesh/status", get(mesh_status))
        .route("/api/mesh/peers", get(mesh_peers))
        .route("/api/mesh/discover", post(mesh_discover))
        .route("/api/mesh/pair/request", post(mesh_pair_request))
        .route("/api/mesh/pair/accept", post(mesh_pair_accept))
        .route("/api/mesh/pair/confirm", post(mesh_pair_confirm))
        .route("/api/mesh/pair/reject", post(mesh_pair_reject))
        .route("/api/mesh/crown", post(mesh_receive_crown))
        .route("/api/mesh/sync", post(mesh_sync))
        .route("/api/mesh/relay", post(mesh_relay))
        .route("/api/mesh/peers/{id}", axum::routing::delete(mesh_unpair))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Resolve a pane ID path parameter: supports both direct IDs and aliases.
fn resolve_pane(state: &AppState, raw_id: &str) -> Result<String, (StatusCode, &'static str)> {
    let mux = state.svc.mux.lock();
    mux.resolve_pane_id(raw_id)
        .ok_or((StatusCode::NOT_FOUND, "Pane not found"))
}

// Auth check helper
fn check_auth(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, &'static str)> {
    if let Some(ref expected) = state.token {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let token = auth.strip_prefix("Bearer ").unwrap_or("");
        if token != expected {
            return Err((StatusCode::UNAUTHORIZED, "Invalid or missing token"));
        }
    }
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "build": env!("BUILD_HASH"),
        "name": "CodeTerm"
    }))
}

async fn identify(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    Ok(Json(serde_json::json!({
        "name": "CodeTerm",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "features": ["multiplexer", "agent-detection", "notifications", "rest-api", "canvas"]
    })))
}

async fn list_workspaces(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mux = state.svc.mux.lock();
    let workspaces = mux.list_workspaces();
    Ok(Json(serde_json::to_value(workspaces).unwrap()))
}

#[derive(Deserialize)]
struct CreateWorkspaceBody {
    name: String,
    #[serde(default)]
    cwd: Option<String>,
}

async fn create_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let ws = state.svc.create_workspace(body.name, body.cwd);
    Ok(Json(serde_json::to_value(ws).unwrap()))
}

async fn focus_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.focus_workspace(&id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct RenameWorkspaceBody {
    name: String,
}

async fn rename_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RenameWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.rename_workspace(&id, &body.name)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn close_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.close_workspace(&id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct MoveTabBody {
    to_workspace_id: String,
}

async fn move_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((workspace_id, tab_id)): Path<(String, String)>,
    Json(body): Json<MoveTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.move_tab(&tab_id, &workspace_id, &body.to_workspace_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to move tab"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn create_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let (tab_id, pane_id) = state.svc.create_tab_with_activate(&workspace_id, false)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
    // API-created tabs are Orchestrators by default, same as Tauri command
    let _ = state.svc.set_pane_role(&pane_id, "Orchestrator", None, Some(true));
    Ok(Json(serde_json::json!({
        "tab_id": tab_id,
        "pane_id": pane_id
    })))
}

#[derive(Deserialize)]
struct SplitPaneBody {
    direction: Option<String>,
}

async fn split_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    body: Option<Json<SplitPaneBody>>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let orientation = match body.as_ref().and_then(|b| b.direction.as_deref()) {
        Some("down" | "vertical") => crate::multiplexer::types::Orientation::Vertical,
        _ => crate::multiplexer::types::Orientation::Horizontal,
    };
    let new_pane_id = state.svc.split_pane(&pane_id, orientation)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    Ok(Json(serde_json::json!({"new_pane_id": new_pane_id})))
}

async fn list_panes(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mux = state.svc.mux.lock();
    let snapshot = mux.snapshot();
    let panes: Vec<_> = snapshot.panes.values().collect();
    Ok(Json(serde_json::to_value(panes).unwrap()))
}

fn default_true() -> bool { true }

#[derive(Deserialize)]
struct SendTextBody {
    text: Option<String>,
    #[serde(default = "default_true")]
    enter: bool,
    /// Batch: send multiple commands sequentially with optional delays
    batch: Option<Vec<BatchCommand>>,
    /// Raw mode: bypass bracket paste, send bytes directly to PTY.
    /// Use for keystrokes that need to be interpreted by TUI (e.g. vim j/k navigation).
    #[serde(default)]
    raw: bool,
}

#[derive(Deserialize)]
struct BatchCommand {
    text: String,
    #[serde(default = "default_true")]
    enter: bool,
    delay_ms: Option<u64>,
}

async fn send_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<SendTextBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;

    // Batch mode
    if let Some(batch) = body.batch {
        let items: Vec<crate::service::pty_io::SendBatchItem> = batch.into_iter().map(|c| {
            crate::service::pty_io::SendBatchItem { text: c.text, enter: c.enter, delay_ms: c.delay_ms }
        }).collect();
        state.svc.send_text_batch(&pane_id, items)
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
        return Ok(Json(serde_json::json!({"ok": true, "batch": true})));
    }

    // Single send
    let text = body.text.unwrap_or_default();
    if body.raw {
        // Raw mode: bypass bracket paste, send directly to PTY
        let mut buf = text;
        if body.enter && !buf.ends_with('\r') { buf.push('\r'); }
        state.svc.write_raw(&pane_id, buf.as_bytes())
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
    } else {
        state.svc.send_text(&pane_id, &text, body.enter)
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn focus_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    state.svc.focus_pane(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct OutputQuery {
    offset: Option<usize>,
    strip_ansi: Option<bool>,
    /// Include current screen text (alternate buffer) in response
    #[allow(dead_code)]
    screen: Option<bool>,
    /// Return full untruncated output history (not just ring buffer)
    full: Option<bool>,
}

async fn get_pane_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Query(query): Query<OutputQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let pty_id = state.svc.resolve_pty_id(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
    let buf = state.svc.output_buf.lock();
    let text = if query.full.unwrap_or(false) {
        buf.get_full_text(&pty_id).unwrap_or_default()
    } else {
        buf.get_text(&pty_id).unwrap_or_default()
    };
    let offset = query.offset.unwrap_or(0);
    let raw = if offset < text.len() { &text[offset..] } else { "" };
    let output = if query.strip_ansi.unwrap_or(false) {
        clean_terminal_output(raw)
    } else {
        raw.to_string()
    };
    Ok(Json(serde_json::json!({
        "output": output,
        "offset": text.len(),
    })))
}

#[derive(Deserialize)]
struct ExecBody {
    command: String,
    timeout_ms: Option<u64>,
    strip_ansi: Option<bool>,
}

async fn exec_in_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<ExecBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let pty_id = state.svc.resolve_pty_id(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;

    let start_offset = state.svc.output_buf.lock()
        .get_text(&pty_id).map(|t| t.len()).unwrap_or(0);

    state.svc.send_text(&pane_id, &body.command, true)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to PTY"))?;

    let timeout_ms = body.timeout_ms.unwrap_or(10000);
    let buf_clone = state.svc.output_buf.clone();
    let pty_clone = pty_id.clone();

    // Poll in a spawned task to avoid Send issues
    let result = tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            let len1 = buf_clone.lock().get_text(&pty_clone).map(|t| t.len()).unwrap_or(0);
            if len1 > start_offset {
                // Wait for output to stabilize (no new data for 800ms)
                tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                let len2 = buf_clone.lock().get_text(&pty_clone).map(|t| t.len()).unwrap_or(0);
                if len2 == len1 || tokio::time::Instant::now() >= deadline {
                    let text = buf_clone.lock().get_text(&pty_clone).unwrap_or_default();
                    let new = if start_offset < text.len() { text[start_offset..].to_string() } else { String::new() };
                    return (new, text.len(), false);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                let text = buf_clone.lock().get_text(&pty_clone).unwrap_or_default();
                let new = if start_offset < text.len() { text[start_offset..].to_string() } else { String::new() };
                return (new, text.len(), true);
            }
        }
    }).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Exec task failed"))?;

    let (raw_output, offset, timed_out) = result;
    let output = if body.strip_ansi.unwrap_or(false) {
        clean_terminal_output(&raw_output)
    } else {
        raw_output
    };
    let mut resp = serde_json::json!({"output": output, "offset": offset});
    if timed_out { resp["timeout"] = serde_json::json!(true); }
    Ok(Json(resp))
}

#[derive(Deserialize)]
struct OpenBrowserBody {
    url: String,
}

async fn open_browser(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<OpenBrowserBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let new_pane_id = state.svc.open_browser(body.url.clone())
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open browser"))?;
    Ok(Json(serde_json::json!({"pane_id": new_pane_id, "url": body.url})))
}

async fn close_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    state.svc.close_pane(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct RenamePaneBody {
    title: String,
}

async fn rename_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<RenamePaneBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    state.svc.rename_pane(&pane_id, &body.title)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct ResizePaneBody {
    cols: u16,
    rows: u16,
}

async fn resize_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<ResizePaneBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    state.svc.resize_pane(&pane_id, body.cols, body.rows)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to resize PTY"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// SSE stream of pane output — real-time output streaming for external agents.
async fn stream_pane_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<axum::response::Sse<impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let pty_id = state.svc.resolve_pty_id(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;

    let buf = state.svc.output_buf.clone();
    let pid = pty_id.clone();
    let mut offset = buf.lock().get_text(&pid).map(|t| t.len()).unwrap_or(0);

    let stream = async_stream::stream! {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            let text = buf.lock().get_text(&pid).unwrap_or_default();
            if text.len() > offset {
                let new_data = text[offset..].to_string();
                offset = text.len();
                yield Ok(axum::response::sse::Event::default()
                    .data(serde_json::json!({"output": new_data, "offset": offset}).to_string()));
            }
        }
    };

    Ok(axum::response::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default()))
}

#[derive(Deserialize)]
struct ListNotifQuery {
    unread: Option<bool>,
}

async fn list_notifications(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListNotifQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let store = state.svc.notif.lock();
    let notifs = store.list(query.unread.unwrap_or(false));
    Ok(Json(serde_json::to_value(notifs).unwrap()))
}

#[derive(Deserialize)]
struct CreateNotifBody {
    title: String,
    body: Option<String>,
    level: Option<String>,
    /// Action buttons for the toast (optional)
    actions: Option<serde_json::Value>,
    /// Keep toast visible until user interacts (optional)
    sticky: Option<bool>,
}

async fn create_notification(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateNotifBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let id = state.svc.create_notification_ext(
        body.title, body.body, body.level.as_deref(),
        body.actions, body.sticky,
    );
    Ok(Json(serde_json::json!({"id": id})))
}

async fn clear_notifications(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.notif.lock().clear(None);
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct ScreenshotQuery {
    format: Option<String>,
}

async fn take_screenshot(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<ScreenshotQuery>,
) -> Result<axum::response::Response, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    let app = state.svc.app_handle.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::screenshot::capture_window(&app)
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Screenshot task failed"))?
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Screenshot capture failed"))?;

    let format = query.format.as_deref().unwrap_or("json");

    match format {
        "jpeg" | "jpg" => {
            // Return raw JPEG bytes
            let b64 = result.data_url
                .strip_prefix("data:image/jpeg;base64,")
                .unwrap_or(&result.data_url);
            let bytes = base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                b64,
            )
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Invalid base64"))?;

            Ok(axum::response::Response::builder()
                .header("Content-Type", "image/jpeg")
                .body(axum::body::Body::from(bytes))
                .unwrap())
        }
        _ => {
            // JSON with data URL + metadata
            let json = serde_json::json!({
                "screenshot": result.data_url,
                "width": result.width,
                "height": result.height,
                "size_kb": result.size_bytes / 1024,
            });
            Ok(axum::response::Response::builder()
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(serde_json::to_string(&json).unwrap()))
                .unwrap())
        }
    }
}

#[derive(Deserialize)]
struct ServeFileQuery {
    path: String,
}

async fn serve_file(
    Query(query): Query<ServeFileQuery>,
) -> Result<axum::response::Response, (StatusCode, &'static str)> {
    let path = std::path::Path::new(&query.path);

    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "File not found"));
    }

    let bytes = std::fs::read(path)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file"))?;

    let content_type = match path.extension().and_then(|e| e.to_str()) {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css",
        Some("js" | "mjs") => "application/javascript",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    };

    Ok(axum::response::Response::builder()
        .header("Content-Type", content_type)
        .body(axum::body::Body::from(bytes))
        .unwrap())
}

// ---------------------------------------------------------------------------
// Screen capture (terminal text content)
// ---------------------------------------------------------------------------

async fn get_pane_screen(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    // Verify pane exists by trying to resolve pty (or at least check mux)
    {
        let mux = state.svc.mux.lock();
        mux.get_pane(&pane_id)
            .ok_or((StatusCode::NOT_FOUND, "Pane not found"))?;
    }

    // Ask frontend to capture screen content via Tauri event
    use tokio::sync::oneshot;
    let (tx, rx) = oneshot::channel::<String>();
    let tx = Arc::new(parking_lot::Mutex::new(Some(tx)));

    let tx_clone = tx.clone();
    let pid = pane_id.clone();

    // Listen for response from frontend
    let app = state.svc.app_handle.clone();
    use tauri::Listener;
    let handler_id = app.listen(format!("screen-capture-response-{}", pid), move |event| {
        if let Some(tx) = tx_clone.lock().take() {
            let payload = event.payload().to_string();
            // Parse the JSON string payload
            let text = serde_json::from_str::<serde_json::Value>(&payload)
                .and_then(|v| Ok(v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string()))
                .unwrap_or_default();
            let _ = tx.send(text);
        }
    });

    // Request capture from frontend
    use tauri::Emitter;
    let _ = state.svc.app_handle.emit("screen-capture-request", serde_json::json!({
        "pane_id": pane_id,
    }));

    // Wait for response with timeout
    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(3),
        rx,
    ).await;

    app.unlisten(handler_id);

    match result {
        Ok(Ok(text)) => Ok(Json(serde_json::json!({
            "screen": text,
            "pane_id": pane_id,
        }))),
        _ => Err((StatusCode::REQUEST_TIMEOUT, "Screen capture timed out")),
    }
}

// ---------------------------------------------------------------------------
// Webhooks
// ---------------------------------------------------------------------------

async fn list_webhooks(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let hooks = state.svc.events.list_webhooks();
    Ok(Json(serde_json::to_value(hooks).unwrap()))
}

#[derive(Deserialize)]
struct RegisterWebhookBody {
    url: String,
    #[serde(default)]
    events: Vec<String>,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    pane_id: Option<String>,
    #[serde(default)]
    once: bool,
    payload_extra: Option<serde_json::Value>,
}

async fn register_webhook(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterWebhookBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let id = uuid::Uuid::new_v4().to_string();
    let reg = WebhookRegistration {
        id: id.clone(),
        url: body.url,
        events: body.events,
        headers: body.headers,
        pane_id: body.pane_id,
        once: body.once,
        payload_extra: body.payload_extra,
    };
    state.svc.events.register_webhook(reg);
    Ok(Json(serde_json::json!({"id": id, "once": body.once})))
}

async fn unregister_webhook(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.events.unregister_webhook(&id);
    Ok(Json(serde_json::json!({"ok": true})))
}

// ---------------------------------------------------------------------------
// Network info
// ---------------------------------------------------------------------------

async fn network_info(
    AxumState(state): AxumState<AppState>,
) -> Json<serde_json::Value> {
    let local_ip = local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    let tailscale_ip = detect_tailscale_ip();
    let port = state.api_port;

    Json(serde_json::json!({
        "local_ip": local_ip,
        "tailscale_ip": tailscale_ip,
        "api_port": port,
        "api_url": format!("http://{}:{}", local_ip, port),
        "tailscale_url": tailscale_ip.as_ref().map(|ip| format!("http://{}:{}", ip, port)),
    }))
}

pub fn local_ip_address() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}

pub fn detect_tailscale_ip() -> Option<String> {
    // Check tailscale status for IP
    let output = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;
    if output.status.success() {
        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !ip.is_empty() { return Some(ip); }
    }
    None
}

// ── Agent Spawn ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SpawnAgentBody {
    /// Agent command to run (e.g. "claude", "aider", "goose"). Default: "claude"
    agent: Option<String>,
    /// Claude agent name from .claude/agents/ (e.g. "architect" → --agent architect)
    agent_name: Option<String>,
    /// Extra args for the agent (e.g. ["--dangerously-skip-permissions", "--agent", "architect"])
    args: Option<Vec<String>>,
    /// Working directory
    cwd: Option<String>,
    /// Tab title (default: agent_name or agent command)
    title: Option<String>,
    /// Role to assign: "Orchestrator", "Lead", "Worker"
    role: Option<String>,
    /// Parent pane ID (required for Worker/SubWorker)
    parent_pane_id: Option<String>,
    /// Allow this agent to spawn children
    can_spawn: Option<bool>,
    /// Task to send after agent starts (optional)
    task: Option<String>,
    /// Workspace ID (default: active workspace)
    workspace_id: Option<String>,
    /// Force creation as a new tab instead of split pane (even with parent)
    force_tab: Option<bool>,
    /// Report target pane ID — for cron agents to report without joining hierarchy
    report_target_pane_id: Option<String>,
    skip_permissions: Option<bool>,
}

async fn spawn_agent(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SpawnAgentBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let params = crate::service::SpawnAgentParams {
        agent: body.agent,
        agent_name: body.agent_name,
        args: body.args,
        cwd: body.cwd,
        title: body.title,
        role: body.role,
        parent_pane_id: body.parent_pane_id,
        can_spawn: body.can_spawn,
        task: body.task,
        workspace_id: body.workspace_id,
        force_tab: body.force_tab,
        report_target_pane_id: body.report_target_pane_id,
            skip_permissions: body.skip_permissions,
    };
    let result = state.svc.spawn_agent(params).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({
        "pane_id": result.pane_id,
        "tab_id": result.tab_id,
        "pty_id": result.pty_id,
        "agent": result.agent,
        "title": result.title,
        "role": result.role,
    })))
}

// ── Batch Commands ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct BatchBody {
    commands: Vec<BatchOp>,
}

#[derive(Deserialize)]
struct BatchOp {
    method: String,
    #[serde(default)]
    params: serde_json::Value,
    /// Optional ID — other commands can reference this result via "$id"
    id: Option<String>,
}

async fn batch_commands(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<BatchBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let ops: Vec<crate::service::BatchOp> = body.commands.into_iter().map(|c| {
        crate::service::BatchOp { method: c.method, params: c.params, id: c.id }
    }).collect();
    let results = state.svc.batch(ops).await;
    Ok(Json(serde_json::json!({"results": results})))
}

// ── Agent Hierarchy Endpoints ────────────────────────────────────────

#[derive(Deserialize)]
struct SetRoleBody {
    role: String,
    parent_pane_id: Option<String>,
    can_spawn: Option<bool>,
}

async fn set_pane_role(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SetRoleBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state.svc.set_pane_role(&id, &body.role, body.parent_pane_id.as_deref(), body.can_spawn)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn clear_pane_role(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state.svc.clear_pane_role(&id).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn get_pane_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let info = state.svc.get_pane_status(&id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::to_value(info).unwrap()))
}

async fn get_pane_children(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let mux = state.svc.mux.lock();
    let child_ids = mux.get_children_ids(&id);
    let children: Vec<serde_json::Value> = child_ids.iter().filter_map(|cid| {
        let p = mux.get_pane(cid)?;
        Some(serde_json::json!({
            "pane_id": p.id,
            "title": p.title,
            "role": p.role,
            "depth": p.depth,
            "agent_type": p.agent_type,
        }))
    }).collect();
    Ok(Json(serde_json::json!(children)))
}

#[derive(Deserialize)]
struct ReportBody {
    message: String,
}

async fn submit_report(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ReportBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let result = state.svc.submit_report(&id, &body.message)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true, "report_id": result.report_id})))
}

#[derive(Deserialize)]
struct ReportsQuery {
    clear: Option<bool>,
}

async fn get_reports(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ReportsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let clear = query.clear.unwrap_or(false);
    let reports = state.svc.report_store.lock().get(&id, clear);
    Ok(Json(serde_json::to_value(&reports).unwrap_or_default()))
}

#[derive(Deserialize)]
struct CanSpawnBody {
    can_spawn: bool,
}

async fn set_can_spawn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CanSpawnBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state.svc.set_can_spawn(&id, body.can_spawn)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn respond_prompt(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let pane_id = resolve_pane(&state, &raw_pane_id)
        .map_err(|(s, m)| (s, m.to_string()))?;
    let value = body.get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing 'value' field".to_string()))?
        .to_string();
    state.svc.respond_prompt(&pane_id, &value)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Clear pending_prompt state without sending to PTY (safe for cleanup/reset).
async fn clear_prompt(
    AxumState(state): AxumState<AppState>,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pane_id = resolve_pane(&state, &raw_pane_id)
        .map_err(|(s, m)| (s, m.to_string()))?;
    state.svc.clear_prompt(&pane_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Hook endpoint: agent reports it needs permission.
/// Called by Claude Code hook (permission_prompt matcher).
/// No auth required — hooks run in the PTY process context.
/// Set agent type on a pane (enables chat view for that agent's provider).
async fn set_agent_type(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let agent_type = body.get("agent_type").and_then(|v| v.as_str()).map(|s| s.to_string());
    state.svc.set_pane_agent_type(&pane_id, agent_type);
    state.svc.emit_state_changed();
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Ensure agent state is set when a hook fires.
/// Hooks are authoritative — if a hook calls us, an agent IS running.
/// This fixes detection on restored panes, Windows (PowerShell doesn't reset title), etc.
fn ensure_agent_state(state: &AppState, pane_id: &str) {
    log::info!("[ensure_agent_state] called for pane {}", pane_id);
    let (agent_type, session_id, cwd) = {
        let mux = state.svc.mux.lock();
        let pane = match mux.get_pane(pane_id) {
            Some(p) => p,
            None => return,
        };
        (pane.agent_type.clone(), pane.agent_session_id.clone(), pane.cwd.clone())
    };

    // Set agent_type if not already set
    let agent_changed = agent_type.is_none();
    if agent_changed {
        // Hook comes from Claude Code hooks → agent is claude
        state.svc.set_pane_agent_type(pane_id, Some("claude".to_string()));
        log::info!("[hook] set agent_type=claude for pane {}", pane_id);
    }

    // Always re-check session — the newest JSONL may have changed since last detection.
    // Hook proves agent is alive, so use a generous 1-hour recency window.
    if let Some(ref cwd) = cwd {
        let exclude = state.svc.mux.lock().other_session_ids(pane_id);
        // Very generous window — hook proves agent is alive, JSONL may be hours old
        // (idle session, long task, user away). 24 hours covers all realistic cases.
        let hook_max_age = Some(std::time::Duration::from_secs(86400));
        log::info!("[ensure_agent_state] detecting session cwd={} current={:?}", cwd, session_id);
        if let Some(newest_id) = state.svc.agent_registry
            .get("claude")
            .and_then(|p| p.detect_session_id(cwd, &exclude, hook_max_age))
        {
            let changed = session_id.as_deref() != Some(newest_id.as_str());
            log::info!("[ensure_agent_state] newest={} changed={}", newest_id, changed);
            if changed {
                state.svc.set_agent_session_id(pane_id, Some(newest_id.clone()));
                log::info!("[hook] session updated: {} -> {} for pane {}",
                    session_id.as_deref().unwrap_or("none"), newest_id, pane_id);
                state.svc.emit_state_changed();
            }
        }
    }

    if agent_changed {
        state.svc.emit_state_changed();
    }
}

/// Accepts optional JSON body with explicit prompt data:
///   { "question": "...", "options": [{"label": "Yes", "value": "1"}, ...] }
/// When body is provided, bypasses ring buffer parsing (useful for testing/external integrations).
async fn hook_permission(
    AxumState(state): AxumState<AppState>,
    Path(raw_pane_id): Path<String>,
    body: Option<Json<serde_json::Value>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pane_id = resolve_pane(&state, &raw_pane_id)
        .map_err(|(s, m)| (s, m.to_string()))?;
    // Hook firing = authoritative proof that an agent is running in this pane.
    // Ensure agent_type is set even if OSC title detection missed it
    // (happens on restored panes, Windows PowerShell not resetting title, etc.)
    ensure_agent_state(&state, &pane_id);

    // Check if explicit prompt data was provided in the request body
    let explicit_prompt = body.and_then(|Json(b)| {
        let question = b.get("question")?.as_str()?.to_string();
        let options = b.get("options")?.as_array()?.iter().filter_map(|o| {
            Some(crate::multiplexer::types::PromptOption {
                label: o.get("label")?.as_str()?.to_string(),
                value: o.get("value")?.as_str()?.to_string(),
            })
        }).collect::<Vec<_>>();
        if options.is_empty() { return None; }
        Some(crate::multiplexer::types::PendingPrompt { question, options })
    });

    let decision = if let Some(prompt) = explicit_prompt {
        // Use explicitly provided prompt data — delegate to PromptDetector
        state.svc.prompt_detector.lock().hook_detected_explicit(&pane_id, prompt)
    } else {
        // Parse prompt from ring buffer via PromptDetector
        let pty_id = state.svc.mux.lock().get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone());
        let agent_type = state.svc.mux.lock().get_pane(&pane_id)
            .and_then(|p| p.agent_type.clone());

        if let Some(pty_id) = pty_id {
            let at = agent_type.unwrap_or_else(|| "claude".to_string());
            let registry = &state.svc.agent_registry;

            // Primary: VT100-parsed screen state (accurate screen content)
            let clean = state.svc.screen_state.lock().get_screen_text(&pty_id).unwrap_or_default();
            log::debug!("[hook_permission] screen_text len={}, agent_type={:?}", clean.len(), at);

            let mut detector = state.svc.prompt_detector.lock();
            let decision = detector.hook_detected_parse(&pane_id, &clean, |text| registry.parse_prompt(&at, text));

            if !matches!(decision, crate::agent::prompt::PromptDecision::None) {
                decision
            } else {
                // Fallback: raw output buffer (preserves UTF-8, contains ANSI).
                // VT100 parser can garble multi-byte chars (C1 byte stripping),
                // so we try the raw buffer with ANSI stripped as a second source.
                let raw = state.svc.output_buf.lock().get_text(&pty_id).unwrap_or_default();
                if raw.is_empty() {
                    decision
                } else {
                    let stripped = strip_ansi(&raw);
                    // Take last ~3000 chars to limit scan scope
                    let tail = if stripped.len() > 3000 { &stripped[stripped.len() - 3000..] } else { &stripped };
                    log::debug!("[hook_permission] fallback raw buffer len={}, tail_len={}", raw.len(), tail.len());
                    detector.hook_detected_parse(&pane_id, tail, |text| registry.parse_prompt(&at, text))
                }
            }
        } else {
            crate::agent::prompt::PromptDecision::None
        }
    };

    // Apply decision (sets prompt on pane, emits events, notifies)
    state.svc.apply_prompt_decision(&decision);

    // For hooks, always notify even if decision is None (hook firing means agent is waiting)
    if matches!(decision, crate::agent::prompt::PromptDecision::None) {
        let title = state.svc.mux.lock().get_pane(&pane_id).map(|p| p.title.clone());
        if let Some(title) = title {
            state.svc.notify_event("agent.permission_request", serde_json::json!({
                "pane_id": &pane_id,
                "title": format!("{} needs approval", title),
                "body": "Agent is waiting for your permission",
            }));
        }
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Hook endpoint: agent stopped (task complete).
/// Called by Claude Code hook (Stop matcher).
async fn hook_stop(
    AxumState(state): AxumState<AppState>,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pane_id = resolve_pane(&state, &raw_pane_id)
        .map_err(|(s, m)| (s, m.to_string()))?;
    let title = state.svc.mux.lock().get_pane(&pane_id).map(|p| p.title.clone());
    if let Some(title) = title {
        state.svc.notify_event("agent.task_complete", serde_json::json!({
            "pane_id": &pane_id,
            "title": format!("{} finished", title),
            "body": "Task complete",
        }));
    }

    // Clear agent_type so next launch gets fresh session detection.
    // BUT: skip for protected panes (General Agent) — agent_type is permanent there.
    let is_protected = state.svc.mux.lock().get_pane(&pane_id).map(|p| p.protected).unwrap_or(false);
    if !is_protected {
        state.svc.set_pane_agent_type(&pane_id, None);
    }
    // Clear any pending prompt left over
    state.svc.prompt_detector.lock().clear(&pane_id);
    {
        let mut mux = state.svc.mux.lock();
        if let Some(pane) = mux.get_pane_mut(&pane_id) {
            pane.pending_prompt = None;
        }
    }
    state.svc.emit_state_changed();

    Ok(Json(serde_json::json!({"ok": true})))
}

async fn list_agents(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    Ok(Json(serde_json::json!(state.svc.list_agents())))
}

async fn get_hierarchy(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    Ok(Json(serde_json::json!(state.svc.get_hierarchy())))
}

/// Strip ANSI escape codes from terminal output.
#[cfg(test)]
mod tests_strip_ansi {
    use super::*;

    #[test]
    fn test_no_ansi() {
        assert_eq!(strip_ansi_codes("hello world"), "hello world");
    }

    #[test]
    fn test_csi_color() {
        assert_eq!(strip_ansi_codes("\x1b[32mgreen\x1b[0m"), "green");
    }

    #[test]
    fn test_osc_title() {
        assert_eq!(strip_ansi_codes("\x1b]0;title\x07text"), "text");
    }

    #[test]
    fn test_osc_st_terminator() {
        assert_eq!(strip_ansi_codes("\x1b]0;title\x1b\\text"), "text");
    }

    #[test]
    fn test_carriage_return_stripped() {
        assert_eq!(strip_ansi_codes("line1\r\nline2"), "line1\nline2");
    }

    #[test]
    fn test_mixed() {
        assert_eq!(
            strip_ansi_codes("\x1b[1;34mBold Blue\x1b[0m and \x1b[31mRed\x1b[0m"),
            "Bold Blue and Red"
        );
    }

    #[test]
    fn test_empty() {
        assert_eq!(strip_ansi_codes(""), "");
    }

    #[test]
    fn test_csi_cursor_move() {
        assert_eq!(strip_ansi_codes("\x1b[2Aup\x1b[3Bdown"), "updown");
    }
}

#[cfg(test)]
mod tests_send_text_body {
    use super::*;

    // ── 2. REST API SendTextBody deserialization ──

    #[test]
    fn send_text_body_enter_defaults_true() {
        let json = r#"{"text": "ls"}"#;
        let body: SendTextBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.text, Some("ls".to_string()));
        assert!(body.enter, "enter should default to true via default_true()");
        assert!(body.batch.is_none());
    }

    #[test]
    fn send_text_body_enter_explicit_false() {
        let json = r#"{"text": "partial", "enter": false}"#;
        let body: SendTextBody = serde_json::from_str(json).unwrap();
        assert!(!body.enter);
    }

    #[test]
    fn send_text_body_enter_explicit_true() {
        let json = r#"{"text": "cmd", "enter": true}"#;
        let body: SendTextBody = serde_json::from_str(json).unwrap();
        assert!(body.enter);
    }

    #[test]
    fn send_text_body_only_text() {
        let json = r#"{"text": "hello"}"#;
        let body: SendTextBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.text, Some("hello".to_string()));
        assert!(body.enter, "enter defaults to true when omitted");
    }

    #[test]
    fn send_text_body_empty_object() {
        let json = r#"{}"#;
        let body: SendTextBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.text, None, "text is None when omitted");
        assert!(body.enter, "enter defaults to true even with empty body");
    }

    #[test]
    fn send_text_body_text_null() {
        let json = r#"{"text": null}"#;
        let body: SendTextBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.text, None);
        assert!(body.enter);
    }

    // ── Batch body tests ──

    #[test]
    fn send_text_body_with_batch() {
        let json = r#"{
            "batch": [
                {"text": "cmd1"},
                {"text": "cmd2", "enter": false},
                {"text": "cmd3", "enter": true, "delay_ms": 500}
            ]
        }"#;
        let body: SendTextBody = serde_json::from_str(json).unwrap();
        let batch = body.batch.unwrap();
        assert_eq!(batch.len(), 3);
        // BatchCommand also uses default_true
        assert!(batch[0].enter, "batch item enter defaults to true");
        assert!(!batch[1].enter);
        assert!(batch[2].enter);
        assert_eq!(batch[2].delay_ms, Some(500));
    }

    #[test]
    fn batch_command_enter_defaults_true() {
        let json = r#"{"text": "echo hi"}"#;
        let cmd: BatchCommand = serde_json::from_str(json).unwrap();
        assert!(cmd.enter, "BatchCommand.enter defaults to true via default_true()");
    }

    // ── 3. ExecBody enter handling ──

    #[test]
    fn exec_body_deserialize() {
        let json = r#"{"command": "ls -la"}"#;
        let body: ExecBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.command, "ls -la");
        assert!(body.timeout_ms.is_none());
        assert!(body.strip_ansi.is_none());
    }

    #[test]
    fn exec_body_with_options() {
        let json = r#"{"command": "echo test", "timeout_ms": 5000, "strip_ansi": true}"#;
        let body: ExecBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.timeout_ms, Some(5000));
        assert_eq!(body.strip_ansi, Some(true));
    }

    // Test the exec enter logic (mirrors exec_in_pane lines 374-378)
    fn apply_exec_enter(command: &str) -> String {
        if command.ends_with('\r') || command.ends_with('\n') {
            command.to_string()
        } else {
            format!("{}\r", command)
        }
    }

    #[test]
    fn exec_command_no_trailing_newline_appends_cr() {
        assert_eq!(apply_exec_enter("ls -la"), "ls -la\r");
    }

    #[test]
    fn exec_command_trailing_cr_no_append() {
        assert_eq!(apply_exec_enter("ls\r"), "ls\r");
    }

    #[test]
    fn exec_command_trailing_newline_no_append() {
        // BASELINE: exec checks BOTH \r and \n — unlike send_text which only checks \r
        assert_eq!(apply_exec_enter("ls\n"), "ls\n");
    }

    #[test]
    fn exec_command_trailing_crlf_no_append() {
        // ends_with('\n') catches this
        assert_eq!(apply_exec_enter("ls\r\n"), "ls\r\n");
    }

    #[test]
    fn exec_empty_command_appends_cr() {
        assert_eq!(apply_exec_enter(""), "\r");
    }

    // ── default_true function ──

    #[test]
    fn default_true_returns_true() {
        assert!(default_true());
    }
}

pub fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // ESC sequence — skip until terminator
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    // CSI sequence: ESC [ ... (letter)
                    chars.next();
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch.is_ascii_alphabetic() || ch == '~' { break; }
                    }
                } else if next == ']' {
                    // OSC sequence: ESC ] ... (BEL or ESC \)
                    chars.next();
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch == '\x07' { break; }
                        if ch == '\x1b' {
                            if chars.peek() == Some(&'\\') { chars.next(); }
                            break;
                        }
                    }
                } else {
                    chars.next(); // skip one char after ESC
                }
            }
        } else if c == '\r' {
            // Skip \r (carriage return) — keep \n only
            continue;
        } else {
            result.push(c);
        }
    }
    result
}

/// Parse raw terminal output through a VT100 emulator to get clean screen text.
/// Clean terminal output using VT100 virtual terminal.
/// Properly handles cursor movements, overwrites (spinners), line wraps, etc.
pub fn clean_terminal_output(raw: &str) -> String {
    let mut parser = vt100::Parser::new(500, 200, 10000);
    parser.process(raw.as_bytes());
    parser.screen().contents()
}

/// Light ANSI strip — removes escape sequences but preserves text layout.
/// Faster than VT100 parser and doesn't reflow text to a different terminal size.
/// Used for prompt detection where we need to find structural patterns
/// in the raw stream regardless of terminal dimensions.
pub fn strip_ansi(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    // CSI: skip until ASCII letter (0x40-0x7E)
                    chars.next();
                    while let Some(&nc) = chars.peek() {
                        chars.next();
                        if nc.is_ascii_alphabetic() || nc == '@' || nc == '~' { break; }
                    }
                }
                Some(']') => {
                    // OSC: skip until BEL or ST (ESC \)
                    chars.next();
                    while let Some(nc) = chars.next() {
                        if nc == '\x07' { break; }
                        if nc == '\x1b' {
                            if chars.peek() == Some(&'\\') { chars.next(); }
                            break;
                        }
                    }
                }
                Some('(' | ')') => { chars.next(); chars.next(); } // charset
                _ => { chars.next(); } // other ESC
            }
        } else if c == '\r' {
            // Skip CR, keep LF
        } else {
            out.push(c);
        }
    }
    out
}

// ── Theme API ──────────────────────────────────────────────────────────

async fn list_themes_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let ids = crate::config::schema::list_all_theme_ids();
    Ok(Json(serde_json::json!({ "themes": ids })))
}

#[derive(Deserialize)]
struct SaveThemeBody {
    id: String,
    toml: String,
}

async fn save_theme_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SaveThemeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let dir = crate::config::schema::user_themes_dir()
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "Cannot determine themes directory"))?;
    std::fs::create_dir_all(&dir).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create themes dir"))?;
    crate::config::schema::parse_builtin_theme(&body.toml)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid theme TOML"))?;
    let path = dir.join(format!("{}.toml", body.id));
    std::fs::write(&path, &body.toml)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write theme file"))?;
    Ok(Json(serde_json::json!({ "ok": true, "path": path.to_string_lossy() })))
}

async fn delete_theme_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    if let Some(dir) = crate::config::schema::user_themes_dir() {
        let path = dir.join(format!("{}.toml", id));
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ApplyThemeBody {
    name: String,
}

async fn apply_theme_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<ApplyThemeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    use tauri::{Emitter, Manager};

    // Get the managed ConfigState from the app handle
    let config_state = state.svc.app_handle.state::<crate::config::ConfigState>();
    let mut config = config_state.lock().clone();
    config.theme.name = body.name.clone();
    if let Some(mut theme) = crate::config::schema::get_any_theme(&body.name) {
        theme.name = body.name;
        config.theme = theme;
    }
    crate::config::save_config_pub(&config)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to save config"))?;
    *config_state.lock() = config.clone();
    let _ = state.svc.app_handle.emit("config-changed", &config);
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Setup endpoints — skills, rules, hooks
// ---------------------------------------------------------------------------

async fn list_skills_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let skills = crate::commands::list_skills()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list skills"))?;
    Ok(Json(serde_json::to_value(skills).unwrap_or_default()))
}

async fn install_skill_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    crate::commands::install_skill(id)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to install skill"))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn uninstall_skill_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    crate::commands::uninstall_skill(id)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to uninstall skill"))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn check_rules_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let status = crate::commands::check_rule_installed()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to check rules"))?;
    Ok(Json(serde_json::to_value(status).unwrap_or_default()))
}

async fn install_rules_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    crate::commands::install_project_rules()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to install rules"))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn check_hooks_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let status = crate::commands::check_claude_hooks()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to check hooks"))?;
    Ok(Json(serde_json::to_value(status).unwrap_or_default()))
}

async fn install_hooks_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    crate::commands::install_claude_hooks(None)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to install hooks"))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn uninstall_hooks_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    crate::commands::uninstall_claude_hooks()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to uninstall hooks"))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Cron API ───────────────────────────────────────────────────────

async fn cron_list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let ws_id = query.get("workspace_id").map(|s| s.as_str());
    let jobs = state.svc.cron_list(&state.cron, ws_id);
    Ok(Json(serde_json::to_value(jobs).unwrap()))
}

async fn cron_get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let job = state.svc.cron_get(&state.cron, &id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

async fn cron_create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<crate::cron::store::CronCreateParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let job = state.svc.cron_create(&state.cron, &state.cron_wake, body)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to create cron job"))?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

async fn cron_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<crate::cron::store::CronUpdateParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let job = state.svc.cron_update(&state.cron, &state.cron_wake, &id, body)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

async fn cron_delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.cron_delete(&state.cron, &state.cron_wake, &id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn cron_enable(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let job = state.svc.cron_enable(&state.cron, &state.cron_wake, &id, true)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

async fn cron_disable(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let job = state.svc.cron_enable(&state.cron, &state.cron_wake, &id, false)
        .map_err(|_| (StatusCode::NOT_FOUND, "Cron job not found"))?;
    Ok(Json(serde_json::to_value(job).unwrap()))
}

async fn cron_trigger(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let run = state.svc.cron_trigger(&state.cron, &id)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to trigger cron job"))?;
    Ok(Json(serde_json::to_value(run).unwrap()))
}

async fn cron_history(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let history = state.svc.cron_history(&state.cron, &id);
    Ok(Json(serde_json::to_value(history).unwrap()))
}

/// Send an OS-level toast notification if the CodeTerm window is not focused.
pub fn send_os_notification_if_unfocused(app: &tauri::AppHandle, title: &str, body: Option<&str>) {
    use tauri::Manager;
    let is_focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(true);

    if !is_focused {
        send_os_notification(app, title, body);
    }
}

/// Single notification popup via system notification API.
/// Uses tauri-plugin-notification on all platforms — clicking focuses the app.
pub fn send_os_notification(app: &tauri::AppHandle, title: &str, body: Option<&str>) {
    // Use tauri plugin on all platforms — produces native notifications
    // that focus the app window when clicked (unlike raw osascript).
    {
        use tauri_plugin_notification::NotificationExt;
        let mut notif = app.notification().builder().title(title);
        if let Some(b) = body { notif = notif.body(b); }
        if notif.show().is_ok() { return; }
    }

    // Keep old custom window code below as last fallback

    #[allow(unreachable_code)]
    {
    use tauri::Manager;
    use std::sync::atomic::{AtomicBool, Ordering};

    static NOTIF_ACTIVE: AtomicBool = AtomicBool::new(false);
    static LAST_MSG: parking_lot::Mutex<String> = parking_lot::Mutex::new(String::new());

    let msg = format!("{}: {}", title, body.unwrap_or(""));

    // Skip duplicate
    {
        let last = LAST_MSG.lock();
        if *last == msg && NOTIF_ACTIVE.load(Ordering::Relaxed) {
            return;
        }
    }
    *LAST_MSG.lock() = msg.clone();

    let title = title.to_string();
    let body = body.unwrap_or("").to_string();
    let app = app.clone();

    // If notification already showing, update its content
    if NOTIF_ACTIVE.load(Ordering::Relaxed) {
        if let Some(win) = app.get_webview_window("codeterm-notif") {
            let html = build_notif_html(&title, &body);
            let _ = win.eval(&format!(
                "document.open();document.write({});document.close();",
                serde_json::to_string(&html).unwrap_or_default()
            ));
            return;
        }
    }

    NOTIF_ACTIVE.store(true, Ordering::Relaxed);

    std::thread::spawn(move || {
        let (wa_x, wa_y, wa_w, wa_h) = get_workarea();
        let win_w = 320.0;
        let win_h = calc_notif_height(&body, win_w);
        let margin = 16.0;

        let html = build_notif_html(&title, &body);

        if let Ok(win) = tauri::WebviewWindowBuilder::new(
            &app,
            "codeterm-notif",
            tauri::WebviewUrl::App("about:blank".into()),
        )
        .title("")
        .inner_size(win_w, win_h)
        .position(wa_x + wa_w - win_w - margin, wa_y + wa_h - win_h - margin)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()
        {
            let _ = win.eval(&format!(
                "document.open();document.write({});document.close();",
                serde_json::to_string(&html).unwrap_or_default()
            ));

            // Auto-close after 4 seconds
            let app_clone = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(4));
                if let Some(w) = app_clone.get_webview_window("codeterm-notif") {
                    let _ = w.close();
                }
                NOTIF_ACTIVE.store(false, Ordering::Relaxed);
            });
        } else {
            NOTIF_ACTIVE.store(false, Ordering::Relaxed);
        }
    });
    } // end unreachable_code block
}

fn build_notif_html(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html><head><style>
        *{{margin:0;padding:0;box-sizing:border-box}}
        body{{background:#1c2128;color:#c9d1d9;font-family:'Segoe UI',sans-serif;
              padding:12px 16px;border:1px solid #30363d;border-radius:8px;
              overflow:hidden;cursor:pointer}}
        body:hover{{background:#21262d}}
        .t{{font-size:13px;font-weight:600;color:#f0f6fc;margin-bottom:4px}}
        .b{{font-size:12px;color:#8b949e}}
        .i{{float:left;margin-right:10px;font-size:18px;color:#f0883e}}
        </style></head><body>
        <span class="i">&#10035;</span>
        <div class="t">{}</div>
        <div class="b">{}</div>
        </body></html>"#,
        title.replace('<', "&lt;").replace('>', "&gt;"),
        body.replace('<', "&lt;").replace('>', "&gt;"),
    )
}

/// Estimate notification popup height based on body text length.
fn calc_notif_height(body: &str, win_w: f64) -> f64 {
    let chars_per_line = ((win_w - 60.0) / 7.0).max(1.0) as usize; // ~7px per char at 12px
    let lines = if body.is_empty() { 0 } else { (body.len() / chars_per_line).max(1) };
    let base = 50.0; // title + padding
    let line_h = 16.0;
    (base + lines as f64 * line_h).max(60.0).min(200.0)
}

/// Returns workarea (x, y, width, height) — desktop minus taskbar.
fn get_workarea() -> (f64, f64, f64, f64) {
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct RECT { left: i32, top: i32, right: i32, bottom: i32 }
        #[link(name = "user32")]
        extern "system" {
            fn SystemParametersInfoW(action: u32, param: u32, pvparam: *mut RECT, winini: u32) -> i32;
        }
        unsafe {
            let mut rect = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
            SystemParametersInfoW(0x0030, 0, &mut rect, 0);
            // Divide by DPI scale — Tauri uses logical pixels
            let dpi = {
                #[link(name = "user32")]
                extern "system" { fn GetDpiForSystem() -> u32; }
                unsafe { GetDpiForSystem() }
            };
            let scale = dpi as f64 / 96.0;
            let x = rect.left as f64 / scale;
            let y = rect.top as f64 / scale;
            let w = (rect.right - rect.left) as f64 / scale;
            let h = (rect.bottom - rect.top) as f64 / scale;
            (x, y, w, h)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        (0.0, 0.0, 1920.0, 1080.0)
    }
}

// ── Update endpoints ──

#[derive(Deserialize)]
struct CheckUpdateQuery {
    force: Option<bool>,
}

async fn check_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<CheckUpdateQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    let force = q.force.unwrap_or(false);

    // Return cached result if fresh (< 1 hour)
    if !force {
        let us = state.update_state.lock();
        if let (Some(ref info), Some(check_time)) = (&us.last_check, us.last_check_time) {
            if check_time.elapsed() < std::time::Duration::from_secs(3600) {
                return Ok(Json(serde_json::to_value(info).unwrap_or_default()));
            }
        }
    }

    let method = state.update_state.lock().install_method.clone();

    let info = crate::update::checker::check_for_update(&method)
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "Failed to check for updates"))?;

    let mut us = state.update_state.lock();
    us.last_check = Some(info.clone());
    us.last_check_time = Some(std::time::Instant::now());

    Ok(Json(serde_json::to_value(info).unwrap_or_default()))
}

async fn install_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    let us = state.update_state.lock();
    let info = us
        .last_check
        .as_ref()
        .ok_or((StatusCode::BAD_REQUEST, "Run check first"))?
        .clone();
    drop(us);

    if !info.update_available {
        return Ok(Json(serde_json::json!({"status": "up_to_date"})));
    }

    match info.update_command {
        Some(cmd) => Ok(Json(serde_json::json!({
            "status": "command",
            "command": cmd,
            "message": format!("Run: {}", cmd)
        }))),
        None => {
            let url = info
                .download_url
                .as_deref()
                .unwrap_or(&info.release_url);
            let _ = crate::commands::open_url(url.to_string());
            Ok(Json(serde_json::json!({
                "status": "browser_opened",
                "url": url,
                "message": "Opened download page in browser"
            })))
        }
    }
}

// ── Config API ──────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct ConfigQuery {
    key: Option<String>,
}

async fn get_config_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ConfigQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let cfg = state.config.lock().clone();
    let full = serde_json::to_value(&cfg)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Config serialization failed"))?;
    match q.key {
        Some(key) => {
            let val = dot_get(&full, &key)
                .ok_or((StatusCode::NOT_FOUND, "Key not found"))?;
            Ok(Json(serde_json::json!({ "key": key, "value": val })))
        }
        None => Ok(Json(full)),
    }
}

#[derive(serde::Deserialize)]
struct PatchConfigBody {
    key: Option<String>,
    value: Option<serde_json::Value>,
    #[serde(flatten)]
    partial: serde_json::Map<String, serde_json::Value>,
}

async fn patch_config_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<PatchConfigBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let cfg = state.config.lock().clone();
    let mut cfg_json = serde_json::to_value(&cfg)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Serialization failed"))?;

    if let (Some(key), Some(value)) = (body.key.as_deref(), &body.value) {
        dot_set(&mut cfg_json, key, value.clone())
            .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid key path"))?;
    } else if !body.partial.is_empty() {
        if let serde_json::Value::Object(ref mut map) = cfg_json {
            for (k, v) in &body.partial {
                if k == "key" || k == "value" { continue; }
                map.insert(k.clone(), v.clone());
            }
        }
    } else {
        return Err((StatusCode::BAD_REQUEST, "Provide key+value or partial config"));
    }

    let new_cfg: crate::config::schema::AppConfig = serde_json::from_value(cfg_json)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid config value"))?;
    *state.config.lock() = new_cfg.clone();
    if let Err(e) = crate::config::save_config_pub(&new_cfg) {
        log::error!("Config save failed: {}", e);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

fn dot_get(val: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut current = val;
    for key in path.split('.') { current = current.get(key)?; }
    Some(current.clone())
}

async fn bridge_send_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let text = body.get("text").and_then(|v| v.as_str())
        .ok_or((StatusCode::BAD_REQUEST, "text required"))?;
    let chat_id = body.get("chat_id").and_then(|v| v.as_i64());

    let tx = state.svc.bridge_outbound.lock();
    let tx = tx.as_ref().ok_or((StatusCode::SERVICE_UNAVAILABLE, "Bridge not active"))?;
    let append = body.get("append").and_then(|v| v.as_bool()).unwrap_or(false);
    let buttons: Option<crate::bridge::traits::InlineKeyboard> = body.get("buttons")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    tx.send(crate::bridge::manager::OutboundMessage {
        user_id: chat_id,
        content: crate::bridge::traits::OutboundContent::Text { text: text.to_string() },
        append,
        buttons,
    }).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Send failed"))?;

    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Mesh networking routes ───────────────────────────────────────────

/// Lightweight health check — no auth, minimal fingerprint.
async fn mesh_ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({"mesh": true}))
}

/// Mesh status — auth required.
async fn mesh_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::to_value(mesh.status()).unwrap()))
}

/// List all peers (paired + discovered + incoming requests) — auth required.
async fn mesh_peers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::to_value(mesh.peers_response()).unwrap()))
}

/// Trigger manual mDNS refresh — auth required.
async fn mesh_discover(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    let discovered: Vec<_> = mesh.discovered_peers.values().cloned().collect();
    Ok(Json(serde_json::to_value(discovered).unwrap()))
}

/// Receive an incoming pairing request — no auth (public endpoint).
async fn mesh_pair_request(
    AxumState(state): AxumState<AppState>,
    Json(request): Json<crate::mesh::types::PairingRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut mesh = state.mesh.lock();
    mesh.receive_pairing_request(request)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    // Emit event so frontend can update the Mesh settings tab
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Receive pairing acceptance from the approver (B→A).
/// Called on the REQUESTER (A) when B approves and sends their accept.
/// A decrypts B's mesh_token, encrypts A's, returns it in the HTTP response.
/// No separate confirm step needed — B reads A's token from the response.
async fn mesh_pair_accept(
    AxumState(state): AxumState<AppState>,
    Json(accept): Json<crate::mesh::types::PairingAccept>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Look up our stored private key for this request
    let our_private_key = {
        let mesh = state.mesh.lock();
        mesh.pairing_keypairs
            .get(&accept.request_id)
            .copied()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "No keypair for this request".to_string()))?
    };

    // Get our outgoing request (stored in mesh_connect) for the nonce
    let outgoing_request = {
        let mesh = state.mesh.lock();
        mesh.pending_requests
            .get(&accept.request_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Original request not found".to_string()))?
    };

    // Decrypt B's mesh token using our private key + B's public key
    let their_mesh_token = crate::mesh::pairing::decrypt_with_shared_secret(
        &our_private_key,
        &accept.public_key,
        &outgoing_request.nonce,
        &accept.encrypted_mesh_token,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, format!("Decrypt failed: {}", e)))?;

    // Generate our mesh token for B and encrypt it — returned in HTTP response
    let our_mesh_token = crate::mesh::pairing::generate_mesh_token();
    let encrypted_our_token = crate::mesh::pairing::encrypt_with_shared_secret(
        &our_private_key,
        &accept.public_key,
        &outgoing_request.nonce,
        &our_mesh_token,
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // Get B's info from the stored pairing target
    let (b_address, b_port, b_instance_id) = {
        let mesh = state.mesh.lock();
        mesh.pairing_targets
            .get(&accept.request_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Target peer info not found".to_string()))?
    };

    // Store B as a paired peer (requester side complete)
    {
        let mut mesh = state.mesh.lock();
        let b_name = mesh.discovered_peers.get(&b_instance_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| b_address.clone());
        let peer = crate::mesh::types::PairedPeer {
            instance_id: b_instance_id.clone(),
            instance_name: b_name,
            addresses: vec![crate::mesh::types::PeerAddress {
                ip: b_address,
                is_tailscale: false,
            }],
            api_port: b_port,
            mesh_token: their_mesh_token,
            peer_public_key: accept.identity_public_key.clone(),
            paired_at: chrono::Utc::now().to_rfc3339(),
            last_seen: Some(chrono::Utc::now().to_rfc3339()),
            online: true,
            is_primary: false,
            access_level: crate::mesh::types::AccessLevel::Full,
            trust_level: String::new(),
            version: None,
        };
        mesh.paired_peers.insert(b_instance_id, peer);
        mesh.pending_requests.remove(&accept.request_id);
        mesh.pairing_keypairs.remove(&accept.request_id);
        mesh.pairing_targets.remove(&accept.request_id);
        let _ = mesh.save();
    }

    log::info!("[mesh] Pairing complete (requester side) for request {}", accept.request_id);
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());

    // Return our encrypted mesh token + identity public key so B can store both
    let our_identity_pubkey = state.mesh.lock().identity.public_key.clone();
    Ok(Json(serde_json::json!({
        "ok": true,
        "encrypted_mesh_token": encrypted_our_token,
        "identity_public_key": our_identity_pubkey
    })))
}

/// Receive crown public info update from a peer.
async fn mesh_receive_crown(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(public_info): Json<crate::mesh::crown::CrownPublicInfo>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Verify mesh token auth
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let from_peer = state.mesh.lock().authenticate_mesh_token(token);
    if from_peer.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "Invalid mesh token".to_string()));
    }

    // If this instance is the new crown holder, it should receive the keypair too
    // For now, just update public info (keypair transfer is TODO for full security)
    state.mesh.lock().update_crown_public(public_info)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Legacy confirm endpoint — kept for compatibility but no longer needed.
/// The new flow returns the requester's token in the accept response.
async fn mesh_pair_confirm(
    AxumState(_state): AxumState<AppState>,
    Json(_confirm): Json<crate::mesh::types::PairingConfirm>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Mesh sync — exchange peer lists and crown info after pairing.
async fn mesh_sync(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(payload): Json<crate::mesh::types::MeshSyncPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Auth: mesh token or API token
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let is_mesh = state.mesh.lock().authenticate_mesh_token(token).is_some();
    let is_api = state.token.as_ref().map(|t| t == token).unwrap_or(true);
    if !is_mesh && !is_api {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }

    let mut mesh = state.mesh.lock();

    // Import discovered peers from sync
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    for peer_info in &payload.peers {
        // Skip ourselves and already-paired peers
        if peer_info.instance_id == mesh.identity.instance_id { continue; }
        if mesh.paired_peers.contains_key(&peer_info.instance_id) { continue; }
        // Add to discovered
        mesh.discovered_peers.insert(
            peer_info.instance_id.clone(),
            crate::mesh::types::DiscoveredPeer {
                instance_id: peer_info.instance_id.clone(),
                instance_name: peer_info.instance_name.clone(),
                addresses: peer_info.addresses.clone(),
                api_port: peer_info.api_port,
                discovery_method: crate::mesh::types::DiscoveryMethod::Mdns, // from gossip
                last_seen: now,
                dev: peer_info.dev,
            },
        );
        log::info!("[mesh/gossip] Learned about peer {} ({})", peer_info.instance_name, peer_info.instance_id);
    }

    // Import crown public info (accept higher version)
    if let Some(crown_info) = payload.crown_public {
        let _ = mesh.update_crown_public(crown_info);
    }

    // Import trust declarations (verify signatures)
    let crown_pubkey = mesh.crown_public.as_ref().map(|c| c.public_key.clone());
    if let Some(pubkey) = crown_pubkey {
        for decl in &payload.trust_declarations {
            if crate::mesh::crown::verify_trust_declaration(decl, &pubkey).is_ok() {
                let existing_ver = mesh.trust_declarations.iter()
                    .filter(|d| d.peer_id == decl.peer_id)
                    .map(|d| d.version)
                    .max()
                    .unwrap_or(0);
                if decl.version > existing_ver {
                    mesh.trust_declarations.retain(|d| d.peer_id != decl.peer_id);
                    mesh.trust_declarations.push(decl.clone());
                }
            }
        }
        let _ = crate::mesh::crown::save_trust_declarations(&mesh.trust_declarations);
    }

    drop(mesh);

    // Return our own sync payload
    let mesh = state.mesh.lock();
    let our_payload = build_sync_payload(&mesh);
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::to_value(our_payload).unwrap()))
}

/// Build sync payload from current mesh state.
fn build_sync_payload(mesh: &crate::mesh::MeshManager) -> crate::mesh::types::MeshSyncPayload {
    let peers: Vec<crate::mesh::types::GossipPeerInfo> = mesh.paired_peers.values()
        .map(|p| crate::mesh::types::GossipPeerInfo {
            instance_id: p.instance_id.clone(),
            instance_name: p.instance_name.clone(),
            addresses: p.addresses.clone(),
            api_port: p.api_port,
            dev: false,
        })
        .collect();
    crate::mesh::types::MeshSyncPayload {
        peers,
        crown_public: mesh.crown_public.clone(),
        trust_declarations: mesh.trust_declarations.clone(),
    }
}

/// Helper: check if sender has trusted access for relay commands.
fn check_mesh_trust(state: &AppState, headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");

    // API token = full access (local)
    if let Some(ref expected) = state.token {
        if token == expected { return Ok("local".to_string()); }
    }

    // Mesh token = check trust level
    let mesh = state.mesh.lock();
    let peer_id = mesh.authenticate_mesh_token(token)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Invalid token".to_string()))?;
    let trust = mesh.get_trust_level(&peer_id);
    match trust {
        crate::mesh::crown::TrustLevel::Trusted => Ok(peer_id),
        _ => {
            // Crown holder can also relay
            if mesh.crown_public.as_ref().map(|c| c.holder_id == peer_id).unwrap_or(false) {
                Ok(peer_id)
            } else {
                Err((StatusCode::FORBIDDEN, format!("Peer {} is {:?} — relay denied", peer_id, trust)))
            }
        }
    }
}

/// Relay — execute commands on this instance from a mesh peer.
async fn mesh_relay(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(cmd): Json<crate::mesh::types::RelayCommand>,
) -> Result<Json<crate::mesh::types::RelayResponse>, (StatusCode, String)> {
    let sender_id = check_mesh_trust(&state, &headers)?;
    log::info!("[mesh/relay] {} from {}", serde_json::to_string(&cmd.action).unwrap_or_default(), sender_id);

    match cmd.action {
        crate::mesh::types::RelayAction::Status => {
            let mesh = state.mesh.lock();
            Ok(Json(crate::mesh::types::RelayResponse {
                ok: true, data: Some(serde_json::to_value(mesh.status()).unwrap()), error: None,
            }))
        }
        crate::mesh::types::RelayAction::Panes => {
            let mux = state.svc.mux.lock();
            let workspaces = mux.list_workspaces();
            Ok(Json(crate::mesh::types::RelayResponse {
                ok: true, data: Some(serde_json::to_value(workspaces).unwrap()), error: None,
            }))
        }
        crate::mesh::types::RelayAction::Output => {
            let pane_id = cmd.pane_id.as_deref()
                .ok_or_else(|| (StatusCode::BAD_REQUEST, "pane_id required".to_string()))?;
            let resolved = resolve_pane(&state, pane_id).map_err(|(s, e)| (s, e.to_string()))?;
            let offset = cmd.params.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let pty_id = {
                let mux = state.svc.mux.lock();
                mux.get_pane(&resolved)
                    .and_then(|p| p.pty_id.clone())
                    .ok_or_else(|| (StatusCode::NOT_FOUND, "Pane has no PTY".to_string()))?
            };
            let buf = state.svc.output_buf.lock();
            let text = buf.get_text(&pty_id).unwrap_or_default();
            Ok(Json(crate::mesh::types::RelayResponse {
                ok: true, data: Some(serde_json::json!({"text": text})), error: None,
            }))
        }
        crate::mesh::types::RelayAction::Send => {
            let pane_id = cmd.pane_id.as_deref()
                .ok_or_else(|| (StatusCode::BAD_REQUEST, "pane_id required".to_string()))?;
            let text = cmd.payload.as_deref().unwrap_or("");
            let resolved = resolve_pane(&state, pane_id).map_err(|(s, e)| (s, e.to_string()))?;
            state.svc.send_text(&resolved, text, true)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
            Ok(Json(crate::mesh::types::RelayResponse {
                ok: true, data: Some(serde_json::json!({"sent": true})), error: None,
            }))
        }
        crate::mesh::types::RelayAction::Exec => {
            let pane_id = cmd.pane_id.as_deref()
                .ok_or_else(|| (StatusCode::BAD_REQUEST, "pane_id required".to_string()))?;
            let command = cmd.payload.as_deref().unwrap_or("");
            let resolved = resolve_pane(&state, pane_id).map_err(|(s, e)| (s, e.to_string()))?;
            // Send command and return immediately (exec with wait requires async — simplify for now)
            state.svc.send_text(&resolved, command, true)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
            Ok(Json(crate::mesh::types::RelayResponse {
                ok: true, data: Some(serde_json::json!({"executed": true})), error: None,
            }))
        }
    }
}

/// Reject a pairing request.
async fn mesh_pair_reject(
    AxumState(state): AxumState<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let request_id = body["request_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing request_id".to_string()))?
        .to_string();

    let mut mesh = state.mesh.lock();
    let block_id = if let Some(req) = mesh.pending_requests.get_mut(&request_id) {
        req.status = crate::mesh::types::PairingStatus::Rejected;
        Some(req.from_instance_id.clone())
    } else {
        None
    };
    if let Some(id) = block_id {
        if !mesh.revocation_list.blocked.contains(&id) {
            mesh.revocation_list.blocked.push(id);
        }
        let _ = mesh.save();
    }
    drop(mesh);

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Unpair from a peer — auth required (local) or mesh token (remote).
async fn mesh_unpair(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Check if this is a remote unpair notification (mesh token) or local (API token)
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");

    let is_local_auth = state
        .token
        .as_ref()
        .map(|t| t == token)
        .unwrap_or(true);
    let is_mesh_auth = state.mesh.lock().authenticate_mesh_token(token).is_some();

    if !is_local_auth && !is_mesh_auth {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }

    let mut mesh = state.mesh.lock();
    if mesh.unpair(&instance_id).is_some() {
        log::info!("[mesh] Unpaired from {}", instance_id);
    }

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

fn dot_set(val: &mut serde_json::Value, path: &str, new_val: serde_json::Value) -> Result<(), ()> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() { return Err(()); }
    let mut current = val;
    for &key in &parts[..parts.len() - 1] { current = current.get_mut(key).ok_or(())?; }
    if let serde_json::Value::Object(ref mut map) = current {
        map.insert(parts.last().unwrap().to_string(), new_val);
        Ok(())
    } else { Err(()) }
}
