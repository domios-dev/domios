use super::{tab_stop::StopDecision, ServiceContext};
use codeterm_types::pty::TabStatusSignal;
use std::sync::Arc;
use std::time::{Duration, Instant};

const STOP_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(10);

fn stop_is_complete(decision: &StopDecision) -> bool {
    matches!(
        decision,
        StopDecision::NoAgent | StopDecision::NotWorking(_)
    )
}

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
            return tokio::task::spawn_blocking(move || check_svc.stop_decision(&check_tab))
                .await
                .is_ok_and(|decision| stop_is_complete(&decision));
        }
        generation = signal.generation();
        let check_svc = svc.clone();
        let check_tab = tab_id.clone();
        match tokio::task::spawn_blocking(move || check_svc.stop_decision(&check_tab)).await {
            Ok(decision) if stop_is_complete(&decision) => return true,
            Ok(_) => {}
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

pub(crate) async fn stop_agent_turn(
    svc: ServiceContext,
    tab_id: String,
) -> Result<serde_json::Value, String> {
    log::info!("[tab-stop] attempt tab={}", tab_id);
    if let Some(result) = crate::service::codex_rpc::control::interrupt(&tab_id).await {
        let stopped = result?;
        return Ok(
            serde_json::json!({"ok":true,"stopped":stopped,"reason":if stopped {"stopped"} else {"interrupt_unconfirmed"},"route":"rpc"}),
        );
    }
    let deadline = Instant::now() + STOP_CONFIRMATION_TIMEOUT;
    let decision_svc = svc.clone();
    let decided_tab = tab_id.clone();
    let decision = tokio::task::spawn_blocking(move || decision_svc.stop_decision(&decided_tab))
        .await
        .map_err(|_| "Stop decision failed".to_string())?;
    match &decision {
        StopDecision::NoAgent => {
            log::info!(
                "[tab-stop] outcome tab={} stopped=false reason=no_agent",
                tab_id
            )
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
            return Ok(serde_json::json!({"ok": true, "stopped": false}));
        }
        StopDecision::NotWorking(status) => {
            return Ok(serde_json::json!({
                "ok": true,
                "stopped": false,
                "reason": "not_working",
                "status": status.label(),
            }));
        }
        StopDecision::Interrupt {
            agent_id,
            session_id,
        } => (agent_id, session_id),
    };
    let pty_id = svc
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.pty_id.clone());
    let (signal, generation) = pty_id
        .as_deref()
        .map(|pty_id| {
            let mut tracker = svc.agent_tracker.lock();
            let signal = tracker.arm_terminal_event_signal(pty_id);
            let generation = signal.generation();
            (Some(signal), Some(generation))
        })
        .unwrap_or((None, None));
    if let Some(sid) = session_id {
        if let Some(pty_id) = pty_id.as_deref() {
            svc.arm_agent_stop_suppression(pty_id);
        }
        let abort_svc = svc.clone();
        let tab = tab_id.clone();
        let structured_abort = tokio::task::spawn_blocking(move || {
            crate::service::structured_prompt::abort_session(&abort_svc, &tab, &sid)
        })
        .await
        .ok()
        .flatten();
        if let Some(Ok(())) = structured_abort {
            if let Some(pty_id) = pty_id.as_deref() {
                svc.mark_agent_interrupted(pty_id);
            }
            if let (Some(signal), Some(generation)) = (signal.clone(), generation) {
                let stopped = stopped_after_status_event(
                    svc.clone(),
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
                return Ok(serde_json::json!({
                    "ok": true,
                    "stopped": stopped,
                    "structured_abort": true,
                    "reason": if stopped { "stopped" } else { "interrupt_unconfirmed" },
                }));
            }
            log::info!(
                "[tab-stop] outcome tab={} stopped=true route=structured_abort",
                tab_id
            );
            return Ok(serde_json::json!({"ok": true, "stopped": true, "structured_abort": true}));
        }
    }
    let stop_config = svc
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
        return Ok(serde_json::json!({
            "ok": true,
            "stopped": false,
            "reason": "stop_unsupported",
        }));
    };
    let screen_signal = if wait_for_output {
        pty_id
            .as_deref()
            .map(|pty_id| svc.screen_state.lock().arm_screen_change_signal(pty_id))
    } else {
        None
    };
    if wait_for_output && screen_signal.is_none() {
        log::warn!(
            "[tab-stop] outcome tab={} stopped=false reason=screen_signal_unavailable",
            tab_id
        );
        return Ok(serde_json::json!({
            "ok": true,
            "stopped": false,
            "reason": "status_signal_unavailable",
        }));
    }
    let mut screen_generation = screen_signal.as_ref().map(|signal| signal.generation());
    if let Some(pty_id) = pty_id.as_deref() {
        svc.arm_agent_stop_suppression(pty_id);
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
                if let Err(error) = svc.write_raw(&tab_id, key.as_bytes()) {
                    log::warn!("[tab-stop] write_failed tab={} error={}", tab_id, error);
                    return Err("Tab not found or has no PTY".to_string());
                }
            }
            StopSequenceStep::ScreenChange => {
                let (Some(signal), Some(generation)) = (screen_signal.clone(), screen_generation)
                else {
                    return Ok(serde_json::json!({
                        "ok": true,
                        "stopped": false,
                        "reason": "status_signal_unavailable",
                    }));
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
                    return Ok(serde_json::json!({
                        "ok": true,
                        "stopped": false,
                        "reason": "interrupt_unconfirmed",
                    }));
                }
                screen_generation = Some(signal.generation());
            }
        }
    }
    if let Some(pty_id) = pty_id.as_deref() {
        svc.mark_agent_interrupted(pty_id);
    }
    let (Some(signal), Some(generation)) = (signal, generation) else {
        log::info!(
            "[tab-stop] outcome tab={} stopped=false reason=status_signal_unavailable",
            tab_id
        );
        return Ok(serde_json::json!({
            "ok": true,
            "stopped": false,
            "reason": "status_signal_unavailable",
        }));
    };
    let stopped =
        stopped_after_status_event(svc.clone(), tab_id.clone(), signal, generation, deadline).await;
    log::info!("[tab-stop] outcome tab={} stopped={}", tab_id, stopped);
    Ok(serde_json::json!({
        "ok": true,
        "stopped": stopped,
        "reason": if stopped { "stopped" } else { "interrupt_unconfirmed" },
    }))
}

#[cfg(test)]
mod tests {
    use super::{stop_is_complete, stop_sequence_steps, StopSequenceStep};

    #[test]
    fn final_stop_snapshot_confirms_idle_without_a_terminal_event() {
        use crate::agent::status::TabStatus;
        use crate::service::tab_stop::StopDecision;

        for decision in [
            StopDecision::NoAgent,
            StopDecision::NotWorking(TabStatus::Idle),
            StopDecision::NotWorking(TabStatus::Dead),
        ] {
            assert!(stop_is_complete(&decision));
        }
        assert!(!stop_is_complete(&StopDecision::Interrupt {
            agent_id: "opaque-provider".into(),
            session_id: Some("own-session".into()),
        }));
    }

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
