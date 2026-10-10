use super::*;
use crate::pty::screen_state::ScreenChangeSignal;
use crate::service::chat_send_transaction::SendEvidenceAnchor;
use std::sync::Arc;

pub(super) struct CommandSequence {
    svc: ServiceContext,
    pty_id: String,
    signal: Arc<ScreenChangeSignal>,
    anchor: Option<SendEvidenceAnchor>,
    deadline: Instant,
    transcript: ModelSwitchTranscript,
}

fn post_command_idle(foreign_turn: bool, idle: bool) -> Result<bool, String> {
    if foreign_turn {
        Err("a foreign user turn intervened between switch commands; switch at its next idle prompt".into())
    } else {
        Ok(idle)
    }
}

#[cfg(test)]
fn wait_for_idle(
    signal: &ScreenChangeSignal,
    deadline: Instant,
    observe: impl FnMut() -> Result<bool, String>,
) -> Result<(), String> {
    wait_for_idle_traced(signal, deadline, observe, |_, _| {})
}

fn wait_for_idle_traced(
    signal: &ScreenChangeSignal,
    deadline: Instant,
    mut observe: impl FnMut() -> Result<bool, String>,
    mut trace_wait: impl FnMut(Duration, bool),
) -> Result<(), String> {
    loop {
        let generation = signal.generation();
        if Instant::now() >= deadline {
            return Err(
                "the confirmed command did not return to idle before the switch deadline".into(),
            );
        }
        if observe()? {
            return Ok(());
        }
        let waiting = Instant::now();
        let changed = signal.wait_after(
            generation,
            deadline.saturating_duration_since(Instant::now()),
        );
        trace_wait(waiting.elapsed(), changed);
        if !changed {
            return Err(if Instant::now() < deadline {
                "the post-command idle event stream closed before the switch deadline"
            } else {
                "the confirmed command did not return to idle before the switch deadline"
            }
            .into());
        }
    }
}

impl CommandSequence {
    pub(super) fn new(
        svc: &ServiceContext,
        target: &CommandTarget,
        deadline: Instant,
        transcript: &ModelSwitchTranscript,
    ) -> Self {
        let signal = command_signal(&mut svc.screen_state.lock(), &target.pty_id);
        svc.restart_tracker
            .lock()
            .staged_models
            .observe_command_idle(&target.tab_id, &signal);
        crate::service::boot_wait::observe_boot_changes(&signal);
        Self {
            svc: svc.clone(),
            pty_id: target.pty_id.clone(),
            signal,
            anchor: svc.chat_send_anchor(&target.tab_id).anchor,
            deadline,
            transcript: transcript.clone(),
        }
    }

    pub(super) fn wait_for_idle(
        &self,
        svc: &ServiceContext,
        target: &CommandTarget,
        unwritten: bool,
    ) -> Result<(), ModelSwitchError> {
        let started = Instant::now();
        let observe_failed = std::cell::Cell::new(false);
        let observe = || {
            if !target.current(svc) || svc.pty.lock().get_session(&target.pty_id).is_none() {
                return Err(
                    "the command session authority changed before post-command idle".into(),
                );
            }
            let anchor = self.anchor.as_ref().ok_or_else(||
                "the next command cannot distinguish foreign activity without an own-session turn boundary".to_owned())?;
            let reading = Instant::now();
            let foreign_turn = crate::service::chat_send_evidence::window_has_foreign_user_turn(
                &svc.agent_registry,
                anchor,
                &self.transcript,
            );
            log::info!("[agent-model/timing] tab={} phase=post_command_record_read elapsed_ms={} succeeded={}", target.tab_id, reading.elapsed().as_millis(), foreign_turn.is_ok());
            let foreign_turn = foreign_turn?;
            if foreign_turn {
                return post_command_idle(true, false);
            }
            let observing = Instant::now();
            let (ready, status) = svc.command_input_ready(target);
            log::info!("[agent-model/timing] tab={} phase=post_command_status_observed elapsed_ms={} status={} declared_frame={} ready={}", target.tab_id, observing.elapsed().as_millis(), status, target.readiness.is_some(), ready);
            post_command_idle(false, ready)
        };
        let result = wait_for_idle_traced(&self.signal, self.deadline, || {
            let observed = observe();
            observe_failed.set(observed.is_err());
            observed
        }, |elapsed, changed| {
            log::info!("[agent-model/timing] tab={} phase=post_command_event_wait elapsed_ms={} changed={}", target.tab_id, elapsed.as_millis(), changed);
        }).map_err(|error| {
            if unwritten && !observe_failed.get() {
                ModelSwitchError::InputNotReady(error)
            } else {
                ModelSwitchError::LiveCommandFailed(error)
            }
        });
        log::info!(
            "[agent-model/timing] tab={} phase=post_command_idle elapsed_ms={} succeeded={}",
            target.tab_id,
            started.elapsed().as_millis(),
            result.is_ok()
        );
        result
    }
}

impl Drop for CommandSequence {
    fn drop(&mut self) {
        self.svc
            .screen_state
            .lock()
            .unsubscribe_screen_changes(&self.pty_id, &self.signal);
    }
}

#[cfg(test)]
mod tests;
