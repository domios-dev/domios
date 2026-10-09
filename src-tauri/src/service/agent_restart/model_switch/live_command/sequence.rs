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

fn wait_for_idle(
    signal: &ScreenChangeSignal,
    deadline: Instant,
    mut observe: impl FnMut() -> Result<bool, String>,
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
        if !signal.wait_after(
            generation,
            deadline.saturating_duration_since(Instant::now()),
        ) {
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
    ) -> Result<(), ModelSwitchError> {
        let started = Instant::now();
        let result = wait_for_idle(&self.signal, self.deadline, || {
            if !target.current(svc) || svc.pty.lock().get_session(&target.pty_id).is_none() {
                return Err("the command session authority changed before post-command idle".into());
            }
            let anchor = self.anchor.as_ref().ok_or_else(||
                "the next command cannot distinguish foreign activity without an own-session turn boundary".to_owned())?;
            let foreign_turn = crate::service::chat_send_evidence::window_has_foreign_user_turn(&svc.agent_registry, anchor, &self.transcript)?;
            post_command_idle(foreign_turn, svc.command_tab_idle(target))
        }).map_err(ModelSwitchError::LiveCommandFailed);
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
