use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_types::pty::TabStatusSignal;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::api::routes::{check_auth, resolve_tab, AppState};
use crate::service::tab_stop::StopDecision;

use super::resolve_pty_id_with_fallback;

const STOP_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(10);

async fn stopped_after_status_event(
    svc: crate::service::ServiceContext,
    tab_id: String,
    signal: Arc<TabStatusSignal>,
    mut generation: u64,
) -> bool {
    let deadline = Instant::now() + STOP_CONFIRMATION_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let wait_signal = signal.clone();
        let changed = tokio::task::spawn_blocking(move || {
            wait_signal.wait_after(generation, remaining)
        })
        .await
        .unwrap_or(false);
        if !changed {
            let check_svc = svc.clone();
            let check_tab = tab_id.clone();
            return matches!(
                tokio::task::spawn_blocking(move || check_svc.stop_decision(&check_tab)).await,
                Ok(StopDecision::NoAgent)
                    | Ok(StopDecision::NotWorking(codeterm_types::pty::TabStatus::Dead))
            );
        }
        generation = signal.generation();
        let check_svc = svc.clone();
        let check_tab = tab_id.clone();
        match tokio::task::spawn_blocking(move || check_svc.stop_decision(&check_tab)).await {
            Ok(StopDecision::Interrupt { .. }) => {}
            Ok(StopDecision::NoAgent | StopDecision::NotWorking(_)) => return true,
            Err(_) => return false,
        }
    }
}

/// Interrupts the agent's turn in progress (not a clean exit) with the
/// provider's declared stop keystroke. An agent with no turn in progress gets
/// no keys and `{stopped:false, reason:"not_working"}`; 404 when the PTY is gone.
pub async fn stop_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    let svc = state.svc.clone();
    let decided_tab = tab_id.clone();
    let decision = tokio::task::spawn_blocking(move || svc.stop_decision(&decided_tab))
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Stop decision failed"))?;
    let (aid, session_id) = match decision {
        StopDecision::NoAgent => {
            return Ok(Json(serde_json::json!({"ok": true, "stopped": false})));
        }
        StopDecision::NotWorking(status) => {
            return Ok(Json(serde_json::json!({
                "ok": true,
                "stopped": false,
                "reason": "not_working",
                "status": status.label(),
            })));
        }
        StopDecision::Interrupt {
            agent_id,
            session_id,
        } => (agent_id, session_id),
    };
    let pty_id = resolve_pty_id_with_fallback(&state, &raw_tab_id).ok();
    let (signal, generation) = pty_id
        .as_deref()
        .map(|pty_id| {
            let mut tracker = state.svc.agent_tracker.lock();
            let signal = tracker.arm_terminal_event_signal(pty_id);
            let generation = signal.generation();
            (Some(signal), Some(generation))
        })
        .unwrap_or((None, None));
    if let Some(sid) = session_id {
        let svc = state.svc.clone();
        let tab = tab_id.clone();
        let structured_abort = tokio::task::spawn_blocking(move || {
            crate::service::structured_prompt::abort_session(&svc, &tab, &sid)
        })
        .await
        .ok()
        .flatten();
        if let Some(Ok(())) = structured_abort {
            if let (Some(signal), Some(generation)) = (signal.clone(), generation) {
                let stopped = stopped_after_status_event(
                    (*state.svc).clone(),
                    tab_id.clone(),
                    signal,
                    generation,
                )
                .await;
                return Ok(Json(serde_json::json!({
                    "ok": true,
                    "stopped": stopped,
                    "structured_abort": true,
                    "reason": if stopped { "stopped" } else { "interrupt_unconfirmed" },
                })));
            }
            return Ok(Json(
                serde_json::json!({"ok": true, "stopped": true, "structured_abort": true}),
            ));
        }
    }
    let key = state
        .svc
        .agent_registry
        .with_provider(&aid, |p| p.stop_keystroke().map(|s| s.to_string()))
        .flatten()
        .filter(|key| !key.is_empty());
    let Some(key) = key else {
        return Ok(Json(serde_json::json!({
            "ok": true,
            "stopped": false,
            "reason": "stop_unsupported",
        })));
    };
    state
        .svc
        .write_raw(&tab_id, key.as_bytes())
        .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found or has no PTY"))?;
    let (Some(signal), Some(generation)) = (signal, generation) else {
        return Ok(Json(serde_json::json!({
            "ok": true,
            "stopped": false,
            "reason": "status_signal_unavailable",
        })));
    };
    let stopped =
        stopped_after_status_event((*state.svc).clone(), tab_id, signal, generation).await;
    Ok(Json(serde_json::json!({
        "ok": true,
        "stopped": stopped,
        "reason": if stopped { "stopped" } else { "interrupt_unconfirmed" },
    })))
}
