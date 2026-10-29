use super::*;
use crate::multiplexer::types::Tab;
use crate::service::boot_stage::{apply_boot_stage, first_turn_input_is_ready, BootStage};
use crate::service::launch_readiness::{attempt_owns_tab, LaunchAttempt};

#[derive(Debug)]
pub(in crate::service::agent_restart) enum SessionlessTeardownError {
    Admission(AgentRestartError),
    Teardown(String),
}

fn eligible(tab: &Tab, snap: &TabRestartSnapshot, attempt: &LaunchAttempt) -> bool {
    snap.session_id.is_none()
        && tab.agent_session_id.is_none()
        && tab.pty_id.as_deref() == Some(snap.pty_id.as_str())
        && tab.agent_type.as_deref() == Some(snap.provider.as_str())
        && attempt_owns_tab(tab, attempt)
        && first_turn_input_is_ready(tab)
        && tab.provisional_launch.as_ref().is_some_and(|launch| {
            launch.first_turn_admitted_send_id.is_none() && !launch.first_turn_payload_written
        })
}

impl ModelSwitchLaunch {
    fn reserve_sessionless_switch(
        &self,
        svc: &ServiceContext,
        snap: &TabRestartSnapshot,
    ) -> Result<LaunchAttempt, AgentRestartError> {
        let refused = || {
            AgentRestartError::BadRequest("first-turn launch changed before model switch".into())
        };
        if !self.is_sessionless()
            || svc
                .agent_registry
                .with_provider(&snap.provider, |provider| {
                    provider.session_starts_on_first_user_turn()
                })
                != Some(true)
        {
            return Err(refused());
        }
        let attempt = self.sessionless_attempt.as_ref().ok_or_else(refused)?;
        let reservation = LaunchAttempt {
            tab_id: attempt.tab_id.clone(),
            pty_id: attempt.pty_id.clone(),
            binding_nonce: uuid::Uuid::new_v4().to_string(),
        };
        {
            let mut mux = svc.mux.write();
            let tab = mux.get_tab_mut(&snap.tab_id).ok_or_else(refused)?;
            if !eligible(tab, snap, attempt) {
                return Err(refused());
            }
            // The send-id claim uses this same lock; retiring its nonce fences every old input path.
            tab.session_binding_nonce = Some(reservation.binding_nonce.clone());
            tab.agent_booting = true;
            apply_boot_stage(tab, BootStage::AgentLaunching, None);
        }
        crate::service::boot_wait::notify_boot_changed();
        svc.emit_state_changed(MuxEmitCause::AgentRestart);
        Ok(reservation)
    }

    pub(in crate::service::agent_restart) fn teardown_sessionless_switch(
        &self,
        svc: &ServiceContext,
        snap: &TabRestartSnapshot,
        teardown: impl FnOnce() -> Result<(), String>,
        commit: impl FnOnce(),
    ) -> Result<(), SessionlessTeardownError> {
        let reservation = self
            .reserve_sessionless_switch(svc, snap)
            .map_err(SessionlessTeardownError::Admission)?;
        if let Err(reason) = teardown() {
            fail_reservation(svc, &reservation, &reason);
            return Err(SessionlessTeardownError::Teardown(reason));
        }
        commit();
        Ok(())
    }
}

fn fail_reservation(svc: &ServiceContext, reservation: &LaunchAttempt, reason: &str) {
    {
        let mut mux = svc.mux.write();
        let Some(tab) = mux.get_tab_mut(&reservation.tab_id) else {
            return;
        };
        if !attempt_owns_tab(tab, reservation) {
            return;
        }
        tab.agent_booting = false;
    }
    svc.record_provisional_launch(
        reservation,
        BootStage::LaunchBindingUnconfirmed,
        &format!("model switch teardown failed; explicit recovery required: {reason}"),
    );
    svc.emit_state_changed(MuxEmitCause::AgentRestart);
}

#[cfg(test)]
mod tests;
