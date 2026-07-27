//! Dev-only cast replay injection into a pane's OutputBuffer (P8 / verification infra).
//! Route is registered only under `cfg(debug_assertions)` — absent in release builds.
//!
//! Body: `{ pane_id, cast_path | cast_b64, instant?, speed?, apply_resize?, classify? }`.
//! `classify:true` is paced-only and feeds chunks through the live PTY status path.

use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use base64::Engine;
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_pane, AppState};
use crate::pty::cast::{self, CastEvent, ParsedCast};

#[derive(Deserialize)]
pub(crate) struct InjectCastBody {
    pane_id: String,
    /// Repo-relative path under `testbench/replays/`.
    cast_path: Option<String>,
    /// Inline asciinema v2 file (base64).
    cast_b64: Option<String>,
    /// Skip timing sleeps and append all output immediately.
    #[serde(default)]
    instant: bool,
    /// Timing multiplier for paced replay (1.0 = real-time).
    #[serde(default = "default_speed")]
    speed: f64,
    /// Apply resize events to the live PTY session.
    #[serde(default)]
    apply_resize: bool,
    /// Also feed paced output chunks through the real PTY status classifier.
    #[serde(default)]
    classify: bool,
}

fn default_speed() -> f64 {
    1.0
}

fn load_cast(body: &InjectCastBody) -> Result<ParsedCast, (StatusCode, String)> {
    match (&body.cast_path, &body.cast_b64) {
        (Some(path), None) => {
            let resolved = cast::resolve_replay_path(&cast::repo_root(), path)
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
            cast::parse_cast_file(&resolved).map_err(|e| (StatusCode::BAD_REQUEST, e))
        }
        (None, Some(b64)) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64.as_bytes())
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("cast_b64: {e}")))?;
            let text = String::from_utf8(bytes)
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("cast_b64 utf8: {e}")))?;
            cast::parse_cast_text(&text).map_err(|e| (StatusCode::BAD_REQUEST, e))
        }
        (Some(_), Some(_)) => Err((
            StatusCode::BAD_REQUEST,
            "provide exactly one of cast_path or cast_b64".into(),
        )),
        (None, None) => Err((
            StatusCode::BAD_REQUEST,
            "cast_path or cast_b64 is required".into(),
        )),
    }
}

fn resolve_target_pty(state: &AppState, raw_pane_id: &str) -> Result<String, (StatusCode, String)> {
    if let Ok(pane_id) = resolve_pane(state, raw_pane_id) {
        if let Ok(pty_id) = state.svc.resolve_pty_id(&pane_id) {
            return Ok(pty_id);
        }
        if let Some(pty_id) = state.svc.agent_run_pty_map.lock().get(&pane_id).cloned() {
            return Ok(pty_id);
        }
    }
    if state.svc.pty.lock().get_session(raw_pane_id).is_some() {
        return Ok(raw_pane_id.to_string());
    }
    if let Some(pty_id) = state.svc.agent_run_pty_map.lock().get(raw_pane_id).cloned() {
        return Ok(pty_id);
    }
    Err((
        StatusCode::NOT_FOUND,
        "Tab not found or has no PTY — use a fixture Tab with a linked PTY".into(),
    ))
}

fn apply_output_chunk(state: &AppState, pty_id: &str, bytes: &[u8], classify: bool) {
    if bytes.is_empty() {
        return;
    }
    if classify {
        let (prev, now) = state.svc.feed_pty_output(pty_id, bytes);
        state.svc.emit_pane_status_if_changed(pty_id, prev, now);
    } else {
        state.svc.output_buf.lock().append(pty_id, bytes);
    }
}

async fn inject_paced(
    state: AppState,
    pty_id: String,
    events: Vec<CastEvent>,
    speed: f64,
    apply_resize: bool,
    classify: bool,
) {
    let speed = speed.max(0.01);
    let mut prev_ms = 0u64;
    for ev in events {
        let at_ms = match &ev {
            CastEvent::Output { at_ms, .. } | CastEvent::Resize { at_ms, .. } => *at_ms,
        };
        if at_ms > prev_ms {
            let delay_ms = ((at_ms - prev_ms) as f64 / speed).round() as u64;
            if delay_ms > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            }
        }
        prev_ms = at_ms;
        match ev {
            CastEvent::Output { bytes, .. } => {
                let svc = state.clone();
                let pid = pty_id.clone();
                let chunk = bytes;
                let _ = tokio::task::spawn_blocking(move || {
                    apply_output_chunk(&svc, &pid, &chunk, classify);
                })
                .await;
            }
            CastEvent::Resize { cols, rows, .. } if apply_resize => {
                let _ = state.svc.pty.lock().resize(&pty_id, cols, rows);
            }
            CastEvent::Resize { .. } => {}
        }
    }
}

/// POST /api/dev/inject-cast — replay cast output into a pane's OutputBuffer.
pub async fn inject_cast(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<InjectCastBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if body.instant && body.classify {
        return Err((
            StatusCode::BAD_REQUEST,
            "classify:true requires paced replay; instant:true would collapse classifier timing"
                .into(),
        ));
    }
    let cast = load_cast(&body)?;
    let pty_id = resolve_target_pty(&state, &body.pane_id)?;
    let planned = cast::output_byte_count(&cast.events);
    let start_offset = state.svc.output_buf.lock().total_written(&pty_id);

    if body.instant {
        let svc = state.svc.clone();
        let pid = pty_id.clone();
        let events = cast.events;
        let (nbytes, end_offset) = tokio::task::spawn_blocking(move || {
            let mut buf = svc.output_buf.lock();
            cast::inject_output_events(&mut buf, &pid, &events)
        })
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("inject join: {e}"),
            )
        })?;
        return Ok(Json(serde_json::json!({
            "ok": true,
            "mode": "instant",
            "pty_id": pty_id,
            "bytes": nbytes,
            "start_offset": start_offset,
            "end_offset": end_offset,
            "planned_bytes": planned,
        })));
    }

    let svc = state.clone();
    let pid = pty_id.clone();
    let events = cast.events;
    let speed = body.speed;
    let apply_resize = body.apply_resize;
    let classify = body.classify;
    tokio::spawn(async move {
        inject_paced(svc, pid, events, speed, apply_resize, classify).await;
    });

    Ok(Json(serde_json::json!({
        "ok": true,
        "mode": "paced",
        "started": true,
        "pty_id": pty_id,
        "start_offset": start_offset,
        "end_offset": start_offset + planned as u64,
        "planned_bytes": planned,
        "speed": body.speed,
        "classify": body.classify,
    })))
}
