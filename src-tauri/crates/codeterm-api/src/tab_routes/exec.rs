use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, resolve_tab, AppState};
use codeterm_service_core::service::ExecWaiter;

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

    let start_offset = state.svc.output_buf.lock().total_written(&pty_id);
    let waiter = ExecWaiter::register(&pty_id);
    let svc = state.svc.clone();
    let spawn_pty = pty_id.clone();
    let command = body.command.clone();
    let command_tab = tab_id.clone();
    tokio::task::spawn_blocking(move || {
        svc.prepare_utility_command(&command_tab, &command);
        svc.send_shell_spawn(&spawn_pty, &command)
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Exec task failed"))?
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to PTY"))?;

    let timeout_ms = body.timeout_ms.unwrap_or(DEFAULT_EXEC_TIMEOUT_MS);
    let completion = waiter
        .wait(std::time::Duration::from_millis(timeout_ms))
        .await;

    let (raw_output, offset, truncated) =
        output_since(&state.svc.output_buf.lock(), &pty_id, start_offset);
    let output = if body.strip_ansi.unwrap_or(false) {
        // ConPTY cursor addresses and shell redraws refer to the real viewport.
        let (cols, rows) = state.svc.pty.lock().size(&pty_id).unwrap_or((200, 50));
        codeterm_service_core::pty::ansi::clean_terminal_output_sized(&raw_output, rows, cols)
    } else {
        raw_output
    };

    let mut resp = serde_json::json!({
        "output": output,
        "offset": offset,
        "exit_code": completion.and_then(|c| c.exit_code()),
        "completion": completion.map_or("timeout", |c| c.label()),
    });
    if truncated {
        log::warn!(
            "[api/exec] output exceeded the retained ring for pty={pty_id}; front truncated"
        );
        resp["output_truncated"] = serde_json::json!(true);
    }
    if completion.is_none() {
        resp["timeout"] = serde_json::json!(true);
        let integrated = state
            .svc
            .ext::<codeterm_service_core::ShellCapabilityState>()
            .lock()
            .integration_observed(&pty_id);
        resp["shell_integration"] = serde_json::json!(integrated);
    }
    Ok(Json(resp))
}

fn tab_shell_exited(state: &AppState, tab_id: &str) -> bool {
    let mux = state.svc.mux.read();
    mux.get_tab(tab_id).is_some_and(|tab| {
        codeterm_service_core::multiplexer::state::effective_tab_status(
            tab,
            state.svc.resolved_tab_status(tab),
        ) == codeterm_service_core::agent::status::TabStatus::Dead
    })
}

// The retained ring stops growing at its cap, so only the monotonic total is a stable cursor;
// fewer bytes than the cursor gap means the front was evicted and the output is incomplete.
fn output_since(
    buf: &codeterm_pty::pty::manager::OutputBuffer,
    pty_id: &str,
    start: u64,
) -> (String, u64, bool) {
    let (bytes, total) = buf.read_since(pty_id, start);
    exec_output(&bytes, total, start)
}

fn exec_output(bytes: &[u8], total: u64, start: u64) -> (String, u64, bool) {
    let truncated = total.saturating_sub(start) > bytes.len() as u64;
    (
        String::from_utf8_lossy(bytes).into_owned(),
        total,
        truncated,
    )
}

#[cfg(test)]
mod tests {
    use super::{exec_output, output_since};
    use codeterm_pty::pty::manager::OutputBuffer;

    #[test]
    fn exec_output_survives_a_full_retained_ring() {
        let mut buf = OutputBuffer::new(64);
        buf.append("pty", &[b'x'; 64]);
        let start = buf.total_written("pty");
        let ring_len_before = buf.get_text("pty").map_or(0, |t| t.len());
        buf.append("pty", b"resource-json");
        assert_eq!(buf.get_text("pty").map_or(0, |t| t.len()), ring_len_before);
        let (output, offset, truncated) = output_since(&buf, "pty", start);
        assert_eq!(output, "resource-json");
        assert_eq!(offset, start + 13);
        assert!(!truncated);
    }

    #[test]
    fn exec_output_with_an_evicted_front_reports_truncation() {
        let (output, offset, truncated) = exec_output(&[b'y'; 16], 140, 100);
        assert_eq!(output.len(), 16);
        assert_eq!(offset, 140);
        assert!(truncated);
        let (_, _, truncated) = exec_output(&[b'y'; 40], 140, 100);
        assert!(!truncated);
    }
}
