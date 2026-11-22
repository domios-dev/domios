use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use codeterm_mesh::contract::delivery;
use codeterm_service_core::service::{ExecCompletion, ShellExecError, ShellExecSubmission};
use serde::Deserialize;

use crate::routes::AppState;

use super::await_pty_link;

const DEFAULT_EXEC_TIMEOUT_MS: u64 = 10_000;

#[derive(Deserialize)]
pub struct ExecBody {
    #[serde(deserialize_with = "codeterm_wire::shell_command::deserialize")]
    pub(crate) command: String,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) strip_ansi: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecRejection {
    pub(crate) status: StatusCode,
    pub(crate) body: serde_json::Value,
}

impl ExecRejection {
    fn refused(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        retry_hint: &'static str,
    ) -> Self {
        let message = message.into();
        Self {
            status,
            body: serde_json::json!({
                "ok": false,
                "outcome": delivery::OUTCOME_REFUSED,
                "bytes_written": false,
                "error": {
                    "code": code,
                    "outcome": delivery::OUTCOME_REFUSED,
                    "bytes_written": false,
                    "message": message,
                    "retry_hint": retry_hint,
                }
            }),
        }
    }

    fn post_write(
        status: StatusCode,
        code: &'static str,
        outcome: &'static str,
        completion: &'static str,
        message: &'static str,
        retry_hint: &'static str,
        observed: ObservedOutput,
    ) -> Self {
        let mut body = serde_json::json!({
            "ok": false,
            "outcome": outcome,
            "bytes_written": true,
            "output": observed.output,
            "offset": observed.offset,
            "exit_code": serde_json::Value::Null,
            "completion": completion,
            "error": {
                "code": code,
                "outcome": outcome,
                "bytes_written": true,
                "message": message,
                "retry_hint": retry_hint,
            }
        });
        if observed.truncated {
            body["output_truncated"] = serde_json::json!(true);
        }
        Self { status, body }
    }

    fn internal(message: &'static str) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: serde_json::json!({
                "ok": false,
                "outcome": delivery::OUTCOME_UNKNOWN,
                "error": {
                    "code": "internal",
                    "outcome": delivery::OUTCOME_UNKNOWN,
                    "message": message,
                }
            }),
        }
    }
}

impl IntoResponse for ExecRejection {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

struct ObservedOutput {
    output: String,
    offset: u64,
    truncated: bool,
}

/// Runs one shell command and answers with its exit code. Completion is the
/// shell's command-finished marker or the shell process exiting; `timeout_ms`
/// is only the failure deadline around that wait.
pub async fn exec_in_tab(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<ExecBody>,
) -> Result<Json<serde_json::Value>, ExecRejection> {
    let tab_id = crate::routes::resolve_tab_async(&state, &raw_tab_id)
        .await
        .map_err(|(status, message)| exec_resolve_rejection(status, message))?;
    let preflight = state.clone();
    let preflight_tab = tab_id.clone();
    tokio::task::spawn_blocking(move || {
        if preflight.svc.tab_has_agent(&preflight_tab) {
            return Err(exec_agent_tab_rejection());
        }
        if tab_shell_exited(&preflight, &preflight_tab) {
            return Err(exec_shell_exited_rejection());
        }
        Ok(())
    })
    .await
    .map_err(|_| ExecRejection::internal("blocking task failed"))??;
    let owner = state.svc.clone();
    let target = tab_id.clone();
    tokio::task::spawn_blocking(move || {
        owner.wake_frozen_tab(&target)?;
        owner.ensure_tab_pty_for_send(&target)
    })
    .await
    .map_err(|_| ExecRejection::internal("Exec wake task failed"))?
    .map_err(|_| exec_missing_pty_rejection("Tab not found or has no PTY"))?;
    if !await_pty_link(&state, &tab_id).await {
        return Err(exec_missing_pty_rejection(
            "Tab not found or PTY did not start",
        ));
    }
    let reader = state.svc.clone();
    let target = tab_id.clone();
    let (pty_id, start_offset) = tokio::task::spawn_blocking(move || {
        let pty_id = reader
            .resolve_pty_id(&target)
            .map_err(|_| exec_missing_pty_rejection("Tab not found or has no PTY"))?;
        let start_offset = reader.output_buf.lock().total_written(&pty_id);
        Ok::<_, ExecRejection>((pty_id, start_offset))
    })
    .await
    .map_err(|_| ExecRejection::internal("blocking task failed"))??;
    let svc = state.svc.clone();
    let spawn_pty = pty_id.clone();
    let command = body.command.clone();
    let strip_ansi = body.strip_ansi.unwrap_or(false);
    let submission =
        tokio::task::spawn_blocking(move || svc.submit_shell_exec(&spawn_pty, &command))
            .await
            .map_err(|_| ExecRejection::internal("Exec task failed"))?
            .map_err(exec_prewrite_rejection)?;

    let waiter = match submission {
        ShellExecSubmission::Started(waiter) => waiter,
        ShellExecSubmission::Unconfirmed { completion, .. } => {
            let observed = read_observed_output(&state, &pty_id, start_offset, strip_ansi).await?;
            return Err(exec_unconfirmed_rejection(observed, completion));
        }
    };

    let timeout_ms = body.timeout_ms.unwrap_or(DEFAULT_EXEC_TIMEOUT_MS);
    let completion = waiter
        .wait(std::time::Duration::from_millis(timeout_ms))
        .await;
    let observed = read_observed_output(&state, &pty_id, start_offset, strip_ansi).await?;
    let integrated = completion.is_none().then(|| {
        state
            .svc
            .ext::<codeterm_service_core::ShellCapabilityState>()
            .lock()
            .integration_observed(&pty_id)
    });
    exec_completion_response(&pty_id, completion, observed, integrated).map(Json)
}

async fn read_observed_output(
    state: &AppState,
    pty_id: &str,
    start_offset: u64,
    strip_ansi: bool,
) -> Result<ObservedOutput, ExecRejection> {
    let reader = state.svc.clone();
    let read_pty = pty_id.to_string();
    tokio::task::spawn_blocking(move || {
        let (raw_output, offset, truncated) =
            output_since(&reader.output_buf.lock(), &read_pty, start_offset);
        let output = if strip_ansi {
            // ConPTY cursor addresses and shell redraws refer to the real viewport.
            let (cols, rows) = reader.pty.lock().size(&read_pty).unwrap_or((200, 50));
            codeterm_service_core::pty::ansi::clean_terminal_output_sized(&raw_output, rows, cols)
        } else {
            raw_output
        };
        ObservedOutput {
            output,
            offset,
            truncated,
        }
    })
    .await
    .map_err(|_| ExecRejection::internal("blocking task failed"))
}

fn exec_completion_response(
    pty_id: &str,
    completion: Option<ExecCompletion>,
    observed: ObservedOutput,
    shell_integration: Option<bool>,
) -> Result<serde_json::Value, ExecRejection> {
    if observed.truncated {
        log::warn!(
            "[api/exec] output exceeded the retained ring for pty={pty_id}; front truncated"
        );
    }
    if matches!(completion, Some(ExecCompletion::TargetRestarting)) {
        return Err(exec_target_restarting_rejection(observed));
    }
    let mut resp = serde_json::json!({
        "output": observed.output,
        "offset": observed.offset,
        "exit_code": completion.and_then(|c| c.exit_code()),
        "completion": completion.map_or("timeout", |c| c.label()),
        "outcome": completion.map_or(delivery::OUTCOME_UNKNOWN, |c| c.outcome()),
        "bytes_written": true,
    });
    if observed.truncated {
        resp["output_truncated"] = serde_json::json!(true);
    }
    if let Some(integrated) = shell_integration {
        resp["timeout"] = serde_json::json!(true);
        resp["shell_integration"] = serde_json::json!(integrated);
    }
    Ok(resp)
}

fn exec_resolve_rejection(status: StatusCode, message: &'static str) -> ExecRejection {
    let code = if status == StatusCode::NOT_FOUND {
        delivery::TAB_NOT_REACHABLE_ON_PEER
    } else {
        delivery::OUTCOME_REFUSED
    };
    ExecRejection::refused(
        status,
        code,
        message,
        "nothing was written — verify the target tab exists",
    )
}

fn exec_agent_tab_rejection() -> ExecRejection {
    ExecRejection::refused(
        StatusCode::CONFLICT,
        delivery::EXEC_AGENT_TAB_REFUSED,
        "exec runs shell commands; use send for an agent tab",
        "nothing was written — target a shell tab or use `domios send` for an agent tab",
    )
}

fn exec_shell_exited_rejection() -> ExecRejection {
    ExecRejection::refused(
        StatusCode::CONFLICT,
        delivery::SHELL_EXITED_REFUSED,
        "the tab's shell has exited; restart it with tab restart",
        "nothing was written — restart the shell with `domios tab restart`, then retry",
    )
}

fn exec_missing_pty_rejection(message: &'static str) -> ExecRejection {
    ExecRejection::refused(
        StatusCode::NOT_FOUND,
        delivery::TAB_NOT_REACHABLE_ON_PEER,
        message,
        "nothing was written — verify the target tab exists and has a live shell PTY",
    )
}

fn exec_prewrite_rejection(error: ShellExecError) -> ExecRejection {
    match error {
        ShellExecError::BusyRefused(detail) => ExecRejection::refused(
            StatusCode::CONFLICT,
            delivery::TAB_BUSY_REFUSED,
            if detail.is_empty() {
                "shell input is busy; nothing written".to_string()
            } else if detail.contains("nothing written") {
                detail
            } else {
                format!("{detail}; nothing written")
            },
            "nothing was written to the target — wait for the active shell command or update to finish, then retry",
        ),
        ShellExecError::PtyUnavailable(detail) => ExecRejection::refused(
            StatusCode::SERVICE_UNAVAILABLE,
            delivery::PTY_UNAVAILABLE,
            if detail.is_empty() {
                "tab has no usable PTY; nothing written".to_string()
            } else {
                format!("{detail}; nothing written")
            },
            "nothing was written — verify the tab has a live shell PTY",
        ),
        ShellExecError::PreparationRefused(detail) => ExecRejection::refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            delivery::OUTCOME_REFUSED,
            detail,
            "nothing was written — adjust the command and retry",
        ),
    }
}

fn exec_unconfirmed_rejection(
    observed: ObservedOutput,
    completion: Option<ExecCompletion>,
) -> ExecRejection {
    let (completion_label, message) = match completion {
        Some(ExecCompletion::TargetRestarting) => (
            "target_restarting",
            "command queued; target daemon restarted before execution was confirmed; do not resend",
        ),
        Some(ExecCompletion::ShellExited { .. }) => (
            "shell_exited",
            "command queued; shell exited before execution was confirmed; do not resend",
        ),
        _ => (
            "submit_unconfirmed",
            "command queued; execution not confirmed; do not resend",
        ),
    };
    ExecRejection::post_write(
        StatusCode::CONFLICT,
        delivery::SUBMIT_UNCONFIRMED,
        delivery::OUTCOME_SUBMIT_UNCONFIRMED,
        completion_label,
        message,
        "the command already reached the target PTY and may have run — inspect the target tab before resending; never replay blindly",
        observed,
    )
}

fn exec_target_restarting_rejection(observed: ObservedOutput) -> ExecRejection {
    ExecRejection::post_write(
        StatusCode::SERVICE_UNAVAILABLE,
        delivery::TARGET_RESTARTING,
        delivery::OUTCOME_UNKNOWN,
        "target_restarting",
        "target daemon restarted mid-exec after command delivery; completion is unknown; do not resend",
        "the command already reached the target PTY before the target restarted — inspect the target after it returns; never replay blindly",
        observed,
    )
}

fn tab_shell_exited(state: &AppState, tab_id: &str) -> bool {
    let mux = state.svc.mux.read();
    mux.get_tab(tab_id).is_some_and(|tab| {
        !tab.frozen
            && codeterm_service_core::multiplexer::state::effective_tab_status(
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
mod tests;
