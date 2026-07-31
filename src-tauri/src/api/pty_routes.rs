//! PTY over HTTP — the browser terminal's spawn/read/write/resize loop.
//! Split out of `web_routes.rs` to keep that file under the size limit.

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use std::time::Duration;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct PtySpawnBody {
    command: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    predicted_id: Option<String>,
    tab_id: Option<String>,
}

/// Web-mirror twin of the `spawn_pty` Tauri command — same launch/resume
/// logic via `spawn_pty_core`, needed for lazy shell spawn and dead-PTY
/// respawn on web (`usePtyConnection.ts`).
pub async fn pty_spawn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<PtySpawnBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let pty_id = crate::commands::spawn_pty_core(
        &state.svc,
        &state.config,
        body.command,
        body.cwd,
        body.cols,
        body.rows,
        body.predicted_id,
        body.tab_id,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "pty_id": pty_id })))
}

#[derive(Deserialize)]
pub(crate) struct PtyTailQuery {
    offset: Option<u64>,
}

/// Browser-safe PTY mirror. Unlike the Tauri `read_pty` command, this never
/// reads the PTY master. It tails bytes already captured in OutputBuffer, so
/// web clients cannot steal terminal output from the desktop webview.
pub async fn pty_read(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Query(query): Query<PtyTailQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let requested_offset = {
        let buf = state.svc.output_buf.lock();
        query.offset.unwrap_or_else(|| buf.total_written(&pty_id))
    };
    let mut offset = requested_offset;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    loop {
        // Message avoids "PTY not found"/"PTY closed" — those trigger frontend respawn.
        if state.svc.pty.lock().get_session(&pty_id).is_none() {
            return Err((StatusCode::NOT_FOUND, "pty session ended".to_string()));
        }
        let (data, next_offset) = state.svc.output_buf.lock().read_since(&pty_id, offset);
        offset = next_offset;
        if !data.is_empty() || tokio::time::Instant::now() >= deadline {
            // ring evicted bytes ahead of it — surface the gap, never hide it.
            let truncated_from =
                (offset - requested_offset > data.len() as u64).then(|| offset - data.len() as u64);
            let mut body = serde_json::json!({ "data": data, "offset": offset });
            if let Some(from) = truncated_from {
                body["truncated_from"] = serde_json::json!(from);
            }
            return Ok(Json(body));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[derive(Deserialize)]
pub(crate) struct PtyWriteBody {
    data: String,
    /// Mirrors the Tauri command: raw=false means "composed text" — bracket
    /// paste + verified Enter for agent tabs. Defaults to true (xterm
    /// keystroke stream).
    #[serde(default = "default_true")]
    raw: bool,
}

fn default_true() -> bool {
    true
}

pub async fn pty_write(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Json(body): Json<PtyWriteBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if body.raw {
        crate::commands::write_pty_string_core(&state.svc, pty_id, body.data)
            .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    } else {
        // off the async runtime.
        let svc = state.svc.clone();
        tokio::task::spawn_blocking(move || {
            let text_stripped = body.data.trim_end_matches('\r');
            let enter = text_stripped.len() != body.data.len();
            svc.send_text_to_pty_input(&pty_id, text_stripped, enter)
        })
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("send task failed: {e}"),
            )
        })?
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct PtyResizeBody {
    cols: u16,
    rows: u16,
    client_id: Option<String>,
}

pub async fn pty_resize(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Json(body): Json<PtyResizeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if let Some(client_id) = body.client_id.as_deref() {
        if !state.resize_owner.lock().is_owner(&pty_id, client_id) {
            return Ok(Json(serde_json::json!({"ok": true, "ignored": true})));
        }
    }
    crate::commands::resize_pty_core(
        &state.svc.pty,
        &state.svc.screen_state,
        pty_id,
        body.cols,
        body.rows,
    )
    .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct ResizeOwnerBody {
    pty_id: String,
    client_id: String,
    cols: u16,
    rows: u16,
    #[serde(default)]
    force: bool,
}

#[derive(Deserialize)]
pub(crate) struct PtyLaunchCommandBody {
    command: String,
}

/// Web-mirror twin of `write_pty_launch_command` — agent boot command via the
/// no-verify launch writer (poll_shell_prompt blocks, so spawn_blocking).
pub async fn pty_launch_command(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Json(body): Json<PtyLaunchCommandBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let command = body.command;
    tokio::task::spawn_blocking(move || {
        svc.send_launch_command_to_pty(&pty_id, &command)
            .map_err(|e| (StatusCode::NOT_FOUND, e))
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("launch task failed: {e}"),
        )
    })??;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn claim_resize_owner(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
    Json(body): Json<ResizeOwnerBody>,
) -> Result<Json<crate::resize_owner::ResizeOwner>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let owner = state.resize_owner.lock().claim(
        tab_id,
        body.pty_id,
        body.client_id,
        body.cols,
        body.rows,
        body.force,
    );
    state
        .events
        .publish(crate::api::events::SystemEvent::Generic {
            event_type: "pty-resize-owner-changed".to_string(),
            data: serde_json::to_value(&owner).unwrap_or_default(),
        });
    Ok(Json(owner))
}
