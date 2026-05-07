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
    let handle = state.svc.spawn_pane(SpawnPaneSpec {
        workspace_id: body.workspace_id,
        cwd: body.cwd,
        title: body.title,
        visibility: PaneVisibility::Visible,
        command: PaneCommand::Shell,
        ..Default::default()
    }).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({
        "pane_id": handle.pane_id,
        "workspace_id": handle.workspace_id,
        "tab_id": handle.tab_id,
    })))
}

fn default_true() -> bool { true }

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

pub async fn send_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<SendTextBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;

    if !await_pty_link(&state, &pane_id).await {
        return Err((StatusCode::NOT_FOUND, "Pane not found or PTY did not start within 3s"));
    }

    if let Some(batch) = body.batch {
        let items: Vec<crate::service::pty_io::SendBatchItem> = batch.into_iter().map(|c| {
            crate::service::pty_io::SendBatchItem { text: c.text, enter: c.enter, delay_ms: c.delay_ms }
        }).collect();
        state.svc.send_text_batch(&pane_id, items)
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
        return Ok(Json(serde_json::json!({"ok": true, "batch": true})));
    }

    let text = body.text.unwrap_or_default();
    if body.raw {
        let mut buf = text;
        if body.enter && !buf.ends_with('\r') { buf.push('\r'); }
        state.svc.write_raw(&pane_id, buf.as_bytes())
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
        return Ok(Json(serde_json::json!({"ok": true})));
    }

    let cmd = crate::dispatch::Command::SendText { pane_id, text, enter: body.enter };
    let result = crate::dispatch::dispatch(&cmd, &state.svc)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
    Ok(Json(result.into_json()))
}

pub async fn focus_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::PaneFocus { pane_id },
        &state.svc,
    )
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
    let pty_id = state.svc.resolve_pty_id(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;

    let start_offset = state.svc.output_buf.lock()
        .get_text(&pty_id).map(|t| t.len()).unwrap_or(0);

    state.svc.send_text(&pane_id, &body.command, true)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to PTY"))?;

    let timeout_ms = body.timeout_ms.unwrap_or(10000);
    let buf_clone = state.svc.output_buf.clone();
    let pty_clone = pty_id.clone();

    let result = tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            let len1 = buf_clone.lock().get_text(&pty_clone).map(|t| t.len()).unwrap_or(0);
            if len1 > start_offset {
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
        crate::pty::ansi::clean_terminal_output(&raw_output)
    } else {
        raw_output
    };
    let mut resp = serde_json::json!({"output": output, "offset": offset});
    if timed_out { resp["timeout"] = serde_json::json!(true); }
    Ok(Json(resp))
}

#[derive(Deserialize)]
pub(crate) struct OpenBrowserBody {
    url: String,
}

pub async fn open_browser(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<OpenBrowserBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let new_pane_id = state.svc.open_browser(body.url.clone())
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open browser"))?;
    Ok(Json(serde_json::json!({"pane_id": new_pane_id, "url": body.url})))
}

pub async fn close_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::PaneClose { pane_id },
        &state.svc,
    )
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
        &crate::dispatch::Command::PaneRename { pane_id, title: body.title },
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
        &crate::dispatch::Command::PtyResize { pane_id, cols: body.cols, rows: body.rows },
        &state.svc,
    )
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to resize PTY"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct StreamQuery {
    from: Option<usize>,
}

pub async fn stream_pane_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<axum::response::Sse<impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pty_id = resolve_pty_id_with_fallback(&state, &raw_pane_id)?;

    let buf = state.svc.output_buf.clone();
    let pid = pty_id.clone();
    let initial_len = buf.lock().get_text(&pid).map(|t| t.len()).unwrap_or(0);
    let mut offset = match query.from {
        Some(n) => n.min(initial_len),
        None => initial_len,
    };

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

pub async fn agent_run_log(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<(StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String), (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;

    if let Ok(pty_id) = resolve_pty_id_with_fallback(&state, &raw_pane_id) {
        let text = state.svc.output_buf.lock().get_full_text(&pty_id).unwrap_or_default();
        if !text.is_empty() {
            return Ok((
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                text,
            ));
        }
    }

    if let Some(text) = read_log_from_disk_by_pane(&state, &raw_pane_id) {
        return Ok((
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
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

// Mux drops pane→pty on close_pane, but output_buf[pty_id] survives — fall
// back to the stable spawn-time map so closed worker/cron logs stay readable.
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

pub async fn get_pane_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Query(query): Query<GetTextQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let cmd = crate::dispatch::Command::GetText { pane_id: pane_id.clone(), formatted: query.formatted };
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
        assert!(cmd.enter, "BatchCommand.enter defaults to true via default_true()");
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
}
