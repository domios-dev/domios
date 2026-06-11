use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_pane, AppState};

pub async fn list_panes(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let result = crate::dispatch::dispatch(&crate::dispatch::Command::PaneList, &state.svc)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list panes"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub struct CreateShellPaneBody {
    pub workspace_id: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
}

pub async fn create_shell_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateShellPaneBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    use crate::service::spawn::{PaneCommand, PaneVisibility, SpawnPaneSpec};
    let handle = state
        .svc
        .spawn_pane(SpawnPaneSpec {
            workspace_id: body.workspace_id,
            cwd: body.cwd,
            title: body.title,
            visibility: PaneVisibility::Visible,
            command: PaneCommand::Shell,
            ..Default::default()
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    // Mirror `pane_id` as `id` so the conventional CRUD idiom (`jq '.id'`, resp.get("id")) reaches the new pane without anyone having to learn our
    // per-resource naming. Both fields hold the same value.
    Ok(Json(serde_json::json!({
        "id": handle.pane_id,
        "pane_id": handle.pane_id,
        "workspace_id": handle.workspace_id,
        "tab_id": handle.tab_id,
    })))
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub(crate) struct SendTextBody {
    text: Option<String>,
    #[serde(default = "default_true")]
    enter: bool,
    /// Batch: send multiple commands sequentially with optional delays
    batch: Option<Vec<BatchCommand>>,
    /// Raw mode: bypass bracket paste, send bytes directly to PTY.
    /// Use for keystrokes that need to be interpreted by TUI (e.g. vim j/k navigation).
    #[serde(default)]
    raw: bool,
    /// Sender's pane id when CLI is invoked from inside CodeTerm. Daemon
    /// prepends a CodeTerm marker so the recipient knows the return-address.
    /// Mesh hops and hierarchy reports use the same marker family.
    #[serde(default)]
    source_pane: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BatchCommand {
    text: String,
    #[serde(default = "default_true")]
    enter: bool,
    delay_ms: Option<u64>,
}

/// PTY init takes ~1-2s after agent_spawn returns; send-text fired
/// immediately would otherwise hit "no PTY" unconditionally. Poll for
/// the link with this short budget so a script can chain spawn → send.
async fn await_pty_link(state: &AppState, pane_id: &str) -> bool {
    for _ in 0..75 {
        if state.svc.resolve_pty_id(pane_id).is_ok() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    false
}

fn record_push_activity(state: &AppState, svc_id: &Option<String>, pane_id: &str, text: &str) {
    use tauri::Emitter;
    let Some(svc_id) = svc_id else { return };
    let writer = crate::service::activity::ActivityWriter::new(state.svc.services_store.conn());
    let preview = text.chars().take(200).collect::<String>();
    let _ = writer.record(crate::service::activity::ActivityEvent::push(
        svc_id, pane_id, &preview,
    ));
    let _ = state.svc.app_handle.emit(
        "service-pushed",
        serde_json::json!({"source_id": svc_id, "target": pane_id}),
    );
}

pub async fn send_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<SendTextBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let svc_token_match = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            crate::service::push::validate_push(v, &state.svc.tokens, &state.svc.services_store)
                .ok()
                .flatten()
        });

    if svc_token_match.is_none() {
        check_auth(&state, &headers)?;
    }
    let pane_id = resolve_pane(&state, &raw_pane_id)?;

    state
        .svc
        .ensure_pane_pty_for_send(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;

    if !await_pty_link(&state, &pane_id).await {
        return Err((
            StatusCode::NOT_FOUND,
            "Pane not found or PTY did not start within 3s",
        ));
    }

    // Prefix only for agent recipients; shells run the bytes verbatim.
    let source_prefix: Option<String> = body
        .source_pane
        .as_deref()
        .filter(|s| !s.is_empty() && *s != pane_id)
        .and_then(|src| {
            let mux = state.svc.mux.lock();
            if mux.get_pane(&pane_id).and_then(|p| p.agent_type.as_ref()).is_none() {
                return None; // shell recipient — no prefix
            }
            let from_label = mux.display_label_for_pane(src);
            let model = mux.get_pane(src).and_then(|p| p.agent_session_model.clone());
            let pfx = crate::service::msg_prefix::CtxPrefix::message(from_label, src).model(model);
            Some(pfx.build())
        });
    let prepend = |s: &str| -> String {
        match &source_prefix {
            Some(p) => format!("{}{}", p, s),
            None => s.to_string(),
        }
    };

    if let Some(batch) = body.batch {
        let preview = batch
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ; ");
        let items: Vec<crate::service::pty_io::SendBatchItem> = batch
            .into_iter()
            .map(|c| crate::service::pty_io::SendBatchItem {
                text: prepend(&c.text),
                enter: c.enter,
                delay_ms: c.delay_ms,
            })
            .collect();
        state
            .svc
            .send_text_batch(&pane_id, items)
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
        record_push_activity(&state, &svc_token_match, &pane_id, &preview);
        return Ok(Json(serde_json::json!({"ok": true, "batch": true})));
    }

    if body.raw {
        // raw mode bypasses bracket paste — sender prefix is intentionally skipped here too (raw = byte-for-byte keystrokes for TUIs)
        let raw_text = body.text.clone().unwrap_or_default();
        let mut buf = raw_text.clone();
        if body.enter && !buf.ends_with('\r') {
            buf.push('\r');
        }
        state
            .svc
            .write_raw(&pane_id, buf.as_bytes())
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
        record_push_activity(&state, &svc_token_match, &pane_id, &raw_text);
        return Ok(Json(serde_json::json!({"ok": true})));
    }

    let text = prepend(&body.text.unwrap_or_default());
    state
        .svc
        .send_text_when_ready(&pane_id, &text, body.enter)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
    record_push_activity(&state, &svc_token_match, &pane_id, &text);
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Interrupt the agent's current operation (NOT a clean exit).
/// Reads `provider.stop_keystroke()` for the pane's agent_type and
/// writes those raw bytes to the PTY. 200 + `{stopped:false}` when
/// the provider has no stop keystroke declared; 404 when the pane
/// or PTY is gone. Used by the per-pane Stop button.
pub async fn stop_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let agent_id = state
        .svc
        .mux
        .lock()
        .get_pane(&pane_id)
        .and_then(|p| p.agent_type.clone());
    let bytes: Option<String> = agent_id.and_then(|aid| {
        state
            .svc
            .agent_registry
            .with_provider(&aid, |p| p.stop_keystroke().map(|s| s.to_string()))
            .flatten()
    });
    let Some(bytes) = bytes else {
        return Ok(Json(serde_json::json!({"ok": true, "stopped": false})));
    };
    state
        .svc
        .write_raw(&pane_id, bytes.as_bytes())
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
    Ok(Json(serde_json::json!({"ok": true, "stopped": true})))
}

pub async fn focus_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let result =
        crate::dispatch::dispatch(&crate::dispatch::Command::PaneFocus { pane_id }, &state.svc)
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct OutputQuery {
    offset: Option<usize>,
    strip_ansi: Option<bool>,
    #[allow(dead_code)]
    screen: Option<bool>,
    full: Option<bool>,
}

pub async fn get_pane_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Query(query): Query<OutputQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let cmd = crate::dispatch::Command::GetOutput {
        pane_id,
        offset: query.offset,
        max_bytes: None,
        strip_ansi: query.strip_ansi.unwrap_or(false),
        full: query.full.unwrap_or(false),
    };
    let result = crate::dispatch::dispatch(&cmd, &state.svc)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct ExecBody {
    command: String,
    timeout_ms: Option<u64>,
    strip_ansi: Option<bool>,
}

pub async fn exec_in_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<ExecBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;

    if !await_pty_link(&state, &pane_id).await {
        return Err((
            StatusCode::NOT_FOUND,
            "Pane not found or PTY did not start within 3s",
        ));
    }
    let pty_id = state
        .svc
        .resolve_pty_id(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;

    let start_offset = state
        .svc
        .output_buf
        .lock()
        .get_text(&pty_id)
        .map(|t| t.len())
        .unwrap_or(0);

    state
        .svc
        .send_text(&pane_id, &body.command, true)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to PTY"))?;

    let timeout_ms = body.timeout_ms.unwrap_or(10000);
    let buf_clone = state.svc.output_buf.clone();
    let pty_clone = pty_id.clone();

    let result = tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            let len1 = buf_clone
                .lock()
                .get_text(&pty_clone)
                .map(|t| t.len())
                .unwrap_or(0);
            if len1 > start_offset {
                tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                let len2 = buf_clone
                    .lock()
                    .get_text(&pty_clone)
                    .map(|t| t.len())
                    .unwrap_or(0);
                if len2 == len1 || tokio::time::Instant::now() >= deadline {
                    let text = buf_clone.lock().get_text(&pty_clone).unwrap_or_default();
                    let new = if start_offset < text.len() {
                        text[start_offset..].to_string()
                    } else {
                        String::new()
                    };
                    return (new, text.len(), false);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                let text = buf_clone.lock().get_text(&pty_clone).unwrap_or_default();
                let new = if start_offset < text.len() {
                    text[start_offset..].to_string()
                } else {
                    String::new()
                };
                return (new, text.len(), true);
            }
        }
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Exec task failed"))?;

    let (raw_output, offset, timed_out) = result;
    let output = if body.strip_ansi.unwrap_or(false) {
        crate::pty::ansi::clean_terminal_output(&raw_output)
    } else {
        raw_output
    };
    let mut resp = serde_json::json!({"output": output, "offset": offset});
    if timed_out {
        resp["timeout"] = serde_json::json!(true);
    }
    Ok(Json(resp))
}

pub async fn close_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let result =
        crate::dispatch::dispatch(&crate::dispatch::Command::PaneClose { pane_id }, &state.svc)
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct RenamePaneBody {
    title: String,
}

pub async fn rename_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<RenamePaneBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::PaneRename {
            pane_id,
            title: body.title,
        },
        &state.svc,
    )
    .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct ResizePaneBody {
    cols: u16,
    rows: u16,
}

pub async fn resize_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<ResizePaneBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::PtyResize {
            pane_id,
            cols: body.cols,
            rows: body.rows,
        },
        &state.svc,
    )
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to resize PTY"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct StreamQuery {
    /// Absolute byte offset (monotonic, survives ring-buffer rotation).
    /// `0` replays the full retained buffer; omitted = start at current tail.
    from: Option<u64>,
}

pub async fn stream_pane_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<
    axum::response::Sse<
        impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
    (StatusCode, &'static str),
> {
    check_auth(&state, &headers)?;
    let pty_id = resolve_pty_id_with_fallback(&state, &raw_pane_id)?;

    let buf = state.svc.output_buf.clone();
    let pid = pty_id.clone();
    let initial_total = buf.lock().total_written(&pid);
    let mut offset: u64 = match query.from {
        Some(n) => n.min(initial_total),
        None => initial_total,
    };

    let stream = async_stream::stream! {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            let (chunk, new_total) = buf.lock().read_since(&pid, offset);
            if !chunk.is_empty() {
                offset = new_total;
                let text = String::from_utf8_lossy(&chunk).to_string();
                yield Ok(axum::response::sse::Event::default()
                    .data(serde_json::json!({"output": text, "offset": offset}).to_string()));
            }
        }
    };

    Ok(axum::response::Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

pub async fn agent_run_log(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<
    (
        StatusCode,
        [(axum::http::header::HeaderName, &'static str); 1],
        String,
    ),
    (StatusCode, &'static str),
> {
    check_auth(&state, &headers)?;

    if let Ok(pty_id) = resolve_pty_id_with_fallback(&state, &raw_pane_id) {
        let text = state
            .svc
            .output_buf
            .lock()
            .get_full_text(&pty_id)
            .unwrap_or_default();
        if !text.is_empty() {
            return Ok((
                StatusCode::OK,
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; charset=utf-8",
                )],
                text,
            ));
        }
    }

    if let Some(text) = read_log_from_disk_by_pane(&state, &raw_pane_id) {
        return Ok((
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            text,
        ));
    }

    Err((StatusCode::NOT_FOUND, "Pane not found or has no PTY"))
}

fn read_log_from_disk_by_pane(state: &AppState, raw_pane_id: &str) -> Option<String> {
    for worker in state.svc.workers.list() {
        if worker.pane_id == raw_pane_id {
            if let Some(ref path) = worker.log_path {
                if let Ok(text) = std::fs::read_to_string(path) {
                    return Some(text);
                }
            }
            break;
        }
    }
    let store = state.cron.lock();
    let jobs: Vec<String> = store.list().iter().map(|j| j.id.clone()).collect();
    for job_id in jobs {
        for run in store.get_history(&job_id) {
            if run.pane_id.as_deref() == Some(raw_pane_id) {
                if let Some(text) = store.read_log(&job_id, &run.run_id) {
                    return Some(text);
                }
            }
        }
    }
    None
}

// close_pane can remove pane→pty, but output_buf[pty_id] may remain.
// Fall back to spawn-time map so worker/cron logs from closed panes stay readable.
fn resolve_pty_id_with_fallback(
    state: &AppState,
    raw_pane_id: &str,
) -> Result<String, (StatusCode, &'static str)> {
    if let Ok(pane_id) = resolve_pane(state, raw_pane_id) {
        if let Ok(pty_id) = state.svc.resolve_pty_id(&pane_id) {
            return Ok(pty_id);
        }
        if let Some(pty_id) = state.svc.agent_run_pty_map.lock().get(&pane_id).cloned() {
            return Ok(pty_id);
        }
    }
    if let Some(pty_id) = state.svc.agent_run_pty_map.lock().get(raw_pane_id).cloned() {
        return Ok(pty_id);
    }
    Err((StatusCode::NOT_FOUND, "Pane not found or has no PTY"))
}

#[derive(Deserialize, Default)]
pub(crate) struct GetTextQuery {
    #[serde(default)]
    formatted: bool,
}

pub async fn get_pane_usage(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;

    let (agent_type, session_id, cwd) = {
        let mux = state.svc.mux.lock();
        let pane = mux
            .get_pane(&pane_id)
            .ok_or((StatusCode::NOT_FOUND, "Pane not found"))?;
        (
            pane.agent_type.clone(),
            pane.agent_session_id.clone(),
            pane.cwd.clone(),
        )
    };

    let (Some(agent_type), Some(session_id), Some(cwd)) = (agent_type, session_id, cwd) else {
        return Ok(Json(
            serde_json::json!({ "usage": serde_json::Value::Null }),
        ));
    };

    let usage = state
        .svc
        .agent_registry
        .with_provider(&agent_type, |p| p.read_usage(&cwd, &session_id))
        .flatten();

    Ok(Json(serde_json::json!({ "usage": usage })))
}

pub async fn get_pane_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Query(query): Query<GetTextQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let cmd = crate::dispatch::Command::GetText {
        pane_id: pane_id.clone(),
        formatted: query.formatted,
    };
    match crate::dispatch::dispatch(&cmd, &state.svc) {
        Ok(r) => {
            let mut v = r.into_json();
            if let Some(obj) = v.as_object_mut() {
                obj.insert("pane_id".to_string(), serde_json::json!(pane_id));
            }
            Ok(Json(v))
        }
        Err(_) => Err((StatusCode::NOT_FOUND, "Pane not found or has no PTY")),
    }
}

#[cfg(test)]
mod tests_send_text_body {
    use super::*;

    #[test]
    fn send_text_body_enter_defaults_true() {
        let json = r#"{"text": "ls"}"#;
        let body: SendTextBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.text, Some("ls".to_string()));
        assert!(
            body.enter,
            "enter should default to true via default_true()"
        );
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
        assert!(batch[0].enter, "batch item enter defaults to true");
        assert!(!batch[1].enter);
        assert!(batch[2].enter);
        assert_eq!(batch[2].delay_ms, Some(500));
    }

    #[test]
    fn batch_command_enter_defaults_true() {
        let json = r#"{"text": "echo hi"}"#;
        let cmd: BatchCommand = serde_json::from_str(json).unwrap();
        assert!(
            cmd.enter,
            "BatchCommand.enter defaults to true via default_true()"
        );
    }

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
        assert_eq!(apply_exec_enter("ls\n"), "ls\n");
    }

    #[test]
    fn exec_command_trailing_crlf_no_append() {
        assert_eq!(apply_exec_enter("ls\r\n"), "ls\r\n");
    }

    #[test]
    fn exec_empty_command_appends_cr() {
        assert_eq!(apply_exec_enter(""), "\r");
    }

    #[test]
    fn default_true_returns_true() {
        assert!(default_true());
    }

    /// send-text is always fire-and-forget — the `verify` field was removed in #101
    /// because TUI agents intercept input and don't echo per-character, making
    /// verification unreliable (especially over mesh). Deserialization must
    /// accept legacy clients that still send the field by ignoring it.
    #[test]
    fn send_text_body_ignores_legacy_verify_field() {
        let json = r#"{"text":"hi","verify":true}"#;
        let body: SendTextBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.text.as_deref(), Some("hi"));
    }
}
