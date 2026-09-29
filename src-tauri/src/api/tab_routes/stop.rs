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
    deadline: Instant,
) -> bool {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let wait_signal = signal.clone();
        let changed =
            tokio::task::spawn_blocking(move || wait_signal.wait_after(generation, remaining))
                .await
                .unwrap_or(false);
        if !changed {
            let check_svc = svc.clone();
            let check_tab = tab_id.clone();
            return matches!(
                tokio::task::spawn_blocking(move || check_svc.stop_decision(&check_tab)).await,
                Ok(StopDecision::NoAgent)
                    | Ok(StopDecision::NotWorking(
                        codeterm_types::pty::TabStatus::Dead
                    ))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopSequenceStep<'a> {
    Key(&'a str),
    ScreenChange,
}

fn stop_sequence_steps(keys: &[String], wait_for_output: bool) -> Vec<StopSequenceStep<'_>> {
    let mut steps = Vec::with_capacity(keys.len() * 2);
    for (index, key) in keys.iter().enumerate() {
        steps.push(StopSequenceStep::Key(key));
        if wait_for_output && index + 1 < keys.len() {
            steps.push(StopSequenceStep::ScreenChange);
        }
    }
    steps
}

/// Interrupts the agent's turn in progress (not a clean exit) with the
/// provider's declared stop-key sequence. An agent with no turn in progress gets
/// no keys and `{stopped:false, reason:"not_working"}`; 404 when the PTY is gone.
pub async fn stop_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    log::info!("[tab-stop] attempt tab={}", tab_id);
    let deadline = Instant::now() + STOP_CONFIRMATION_TIMEOUT;
    let svc = state.svc.clone();
    let decided_tab = tab_id.clone();
    let decision = tokio::task::spawn_blocking(move || svc.stop_decision(&decided_tab))
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Stop decision failed"))?;
    match &decision {
        StopDecision::NoAgent => {
            log::info!("[tab-stop] outcome tab={} stopped=false reason=no_agent", tab_id)
        }
        StopDecision::NotWorking(status) => log::info!(
            "[tab-stop] outcome tab={} stopped=false reason=not_working status={}",
            tab_id,
            status.label()
        ),
        StopDecision::Interrupt { agent_id, .. } => {
            log::info!("[tab-stop] decision tab={} provider={}", tab_id, agent_id)
        }
    }
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
        if let Some(pty_id) = pty_id.as_deref() {
            state.svc.arm_agent_stop_suppression(pty_id);
        }
        let svc = state.svc.clone();
        let tab = tab_id.clone();
        let structured_abort = tokio::task::spawn_blocking(move || {
            crate::service::structured_prompt::abort_session(&svc, &tab, &sid)
        })
        .await
        .ok()
        .flatten();
        if let Some(Ok(())) = structured_abort {
            if let Some(pty_id) = pty_id.as_deref() {
                state.svc.mark_agent_interrupted(pty_id);
            }
            if let (Some(signal), Some(generation)) = (signal.clone(), generation) {
                let stopped = stopped_after_status_event(
                    (*state.svc).clone(),
                    tab_id.clone(),
                    signal,
                    generation,
                    deadline,
                )
                .await;
                log::info!(
                    "[tab-stop] outcome tab={} stopped={} route=structured_abort",
                    tab_id,
                    stopped
                );
                return Ok(Json(serde_json::json!({
                    "ok": true,
                    "stopped": stopped,
                    "structured_abort": true,
                    "reason": if stopped { "stopped" } else { "interrupt_unconfirmed" },
                })));
            }
            log::info!(
                "[tab-stop] outcome tab={} stopped=true route=structured_abort",
                tab_id
            );
            return Ok(Json(
                serde_json::json!({"ok": true, "stopped": true, "structured_abort": true}),
            ));
        }
    }
    let stop_config = state
        .svc
        .agent_registry
        .with_provider(&aid, |provider| {
            provider
                .stop_keys()
                .map(|keys| (keys, provider.stop_key_wait_for_output()))
        })
        .flatten()
        .filter(|(keys, _)| !keys.is_empty() && keys.iter().all(|key| !key.is_empty()));
    let Some((keys, wait_for_output)) = stop_config else {
        log::info!(
            "[tab-stop] outcome tab={} stopped=false reason=stop_unsupported",
            tab_id
        );
        return Ok(Json(serde_json::json!({
            "ok": true,
            "stopped": false,
            "reason": "stop_unsupported",
        })));
    };
    let screen_signal = if wait_for_output {
        pty_id.as_deref().map(|pty_id| {
            state
                .svc
                .screen_state
                .lock()
                .arm_screen_change_signal(pty_id)
        })
    } else {
        None
    };
    if wait_for_output && screen_signal.is_none() {
        log::warn!(
            "[tab-stop] outcome tab={} stopped=false reason=screen_signal_unavailable",
            tab_id
        );
        return Ok(Json(serde_json::json!({
            "ok": true,
            "stopped": false,
            "reason": "status_signal_unavailable",
        })));
    }
    let mut screen_generation = screen_signal.as_ref().map(|signal| signal.generation());
    if let Some(pty_id) = pty_id.as_deref() {
        state.svc.arm_agent_stop_suppression(pty_id);
    }
    log::info!(
        "[tab-stop] sequence tab={} keys={} wait_for_output={}",
        tab_id,
        keys.len(),
        wait_for_output
    );
    for step in stop_sequence_steps(&keys, wait_for_output) {
        match step {
            StopSequenceStep::Key(key) => {
                if let Err(error) = state.svc.write_raw(&tab_id, key.as_bytes()) {
                    log::warn!(
                        "[tab-stop] write_failed tab={} error={}",
                        tab_id,
                        error
                    );
                    return Err((StatusCode::NOT_FOUND, "Tab not found or has no PTY"));
                }
            }
            StopSequenceStep::ScreenChange => {
                let (Some(signal), Some(generation)) = (screen_signal.clone(), screen_generation)
                else {
                    return Ok(Json(serde_json::json!({
                        "ok": true,
                        "stopped": false,
                        "reason": "status_signal_unavailable",
                    })));
                };
                let remaining = deadline.saturating_duration_since(Instant::now());
                let wait_signal = signal.clone();
                let changed = !remaining.is_zero()
                    && tokio::task::spawn_blocking(move || {
                        wait_signal.wait_after(generation, remaining)
                    })
                    .await
                    .unwrap_or(false);
                if !changed {
                    log::warn!(
                        "[tab-stop] outcome tab={} stopped=false reason=interrupt_unconfirmed sequence_step=screen_change",
                        tab_id
                    );
                    return Ok(Json(serde_json::json!({
                        "ok": true,
                        "stopped": false,
                        "reason": "interrupt_unconfirmed",
                    })));
                }
                screen_generation = Some(signal.generation());
            }
        }
    }
    if let Some(pty_id) = pty_id.as_deref() {
        state.svc.mark_agent_interrupted(pty_id);
    }
    let (Some(signal), Some(generation)) = (signal, generation) else {
        log::info!(
            "[tab-stop] outcome tab={} stopped=false reason=status_signal_unavailable",
            tab_id
        );
        return Ok(Json(serde_json::json!({
            "ok": true,
            "stopped": false,
            "reason": "status_signal_unavailable",
        })));
    };
    let stopped = stopped_after_status_event(
        (*state.svc).clone(),
        tab_id.clone(),
        signal,
        generation,
        deadline,
    )
    .await;
    log::info!("[tab-stop] outcome tab={} stopped={}", tab_id, stopped);
    Ok(Json(serde_json::json!({
        "ok": true,
        "stopped": stopped,
        "reason": if stopped { "stopped" } else { "interrupt_unconfirmed" },
    })))
}

#[cfg(test)]
mod tests {
    use super::{stop_sequence_steps, StopSequenceStep};

    #[test]
    fn stop_sequence_writes_each_key_with_the_declared_screen_gap() {
        let keys = vec!["\x1b".to_string(), "\x1b".to_string()];
        assert_eq!(
            stop_sequence_steps(&keys, true),
            vec![
                StopSequenceStep::Key("\x1b"),
                StopSequenceStep::ScreenChange,
                StopSequenceStep::Key("\x1b"),
            ]
        );
    }

    #[test]
    fn single_stop_key_has_no_inter_key_wait() {
        let keys = vec!["\x1b".to_string()];
        assert_eq!(
            stop_sequence_steps(&keys, true),
            vec![StopSequenceStep::Key("\x1b")]
        );
    }
}
