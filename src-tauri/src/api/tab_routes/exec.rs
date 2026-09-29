use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_tab, AppState};
use crate::service::shell_exec::ExecWaiter;

use super::await_pty_link;

const DEFAULT_EXEC_TIMEOUT_MS: u64 = 10_000;

#[derive(Deserialize)]
pub(crate) struct ExecBody {
    pub(crate) command: String,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) strip_ansi: Option<bool>,
}

/// Runs one shell command and answers with its exit code. Completion is the
/// shell's command-finished marker or the shell process exiting; `timeout_ms`
/// is only the failure deadline around that wait.
pub async fn exec_in_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<ExecBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    if state.svc.tab_has_agent(&tab_id) {
        return Err((
            StatusCode::CONFLICT,
            "exec runs shell commands; use send for an agent tab",
        ));
    }
    if tab_shell_exited(&state, &tab_id) {
        return Err((
            StatusCode::CONFLICT,
            "the tab's shell has exited; restart it with tab restart",
        ));
    }

    state
        .svc
        .ensure_tab_pty_for_send(&tab_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found or has no PTY"))?;
    if !await_pty_link(&state, &tab_id).await {
        return Err((StatusCode::NOT_FOUND, "Tab not found or PTY did not start"));
    }
    let pty_id = state
        .svc
        .resolve_pty_id(&tab_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found or has no PTY"))?;

    let start_offset = output_len(&state, &pty_id);
    let waiter = ExecWaiter::register(&pty_id);
    let svc = state.svc.clone();
    let spawn_pty = pty_id.clone();
    let command = body.command.clone();
    tokio::task::spawn_blocking(move || svc.send_shell_spawn(&spawn_pty, &command))
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Exec task failed"))?
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to PTY"))?;

    let timeout_ms = body.timeout_ms.unwrap_or(DEFAULT_EXEC_TIMEOUT_MS);
    let completion = waiter
        .wait(std::time::Duration::from_millis(timeout_ms))
        .await;

    let text = state
        .svc
        .output_buf
        .lock()
        .get_text(&pty_id)
        .unwrap_or_default();
    let offset = text.len();
    let raw_output = text.get(start_offset..).unwrap_or_default().to_string();
    let output = if body.strip_ansi.unwrap_or(false) {
        // Re-emulate at the pty's REAL width: shell partial-line redraws
        // (zsh's % padding) erase the last output line at any other width.
        let cols = state
            .svc
            .pty
            .lock()
            .size(&pty_id)
            .map(|(c, _)| c)
            .unwrap_or(200);
        crate::pty::ansi::clean_terminal_output_sized(&raw_output, 500, cols)
    } else {
        raw_output
    };

    let mut resp = serde_json::json!({
        "output": output,
        "offset": offset,
        "exit_code": completion.and_then(|c| c.exit_code()),
        "completion": completion.map_or("timeout", |c| c.label()),
    });
    if completion.is_none() {
        resp["timeout"] = serde_json::json!(true);
        let integrated = state.svc.shell_caps.lock().integration_observed(&pty_id);
        resp["shell_integration"] = serde_json::json!(integrated);
    }
    Ok(Json(resp))
}

fn tab_shell_exited(state: &AppState, tab_id: &str) -> bool {
    let mux = state.svc.mux.read();
    mux.get_tab(tab_id).is_some_and(|tab| {
        crate::multiplexer::state::effective_tab_status(tab, state.svc.resolved_tab_status(tab))
            == crate::agent::status::TabStatus::Dead
    })
}

fn output_len(state: &AppState, pty_id: &str) -> usize {
    state
        .svc
        .output_buf
        .lock()
        .get_text(pty_id)
        .map(|text| text.len())
        .unwrap_or(0)
}
