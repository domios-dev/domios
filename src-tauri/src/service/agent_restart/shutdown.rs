use super::{ServiceContext, TabRestartSnapshot};
use std::time::Duration;

impl ServiceContext {
    pub(super) fn request_graceful_exit(
        &self,
        snap: &TabRestartSnapshot,
        force: bool,
        grace: Duration,
    ) {
        if force {
            return;
        }
        if self.idle_restart_termination_allowed(snap) {
            crate::service::agent_restart::timing::stage(
                &snap.tab_id,
                &snap.pty_id,
                "idle-terminate",
            );
            return;
        }
        let exit_command = self
            .agent_registry
            .with_provider(&snap.provider, |provider| {
                provider.exit_command().map(str::to_string)
            })
            .flatten();
        let Some(exit_command) = exit_command else {
            return;
        };
        // A busy composer takes the exit command as a queued user turn, so a
        // working agent is stopped at the process by the PTY teardown instead.
        if !self.tab_is_idle(&snap.tab_id) {
            log::info!(
                "[agent_restart] tab={} is busy; skipping the in-chat exit command",
                snap.tab_id
            );
            return;
        }
        let claim =
            crate::service::pty_io::submit_transaction::claim_exit_submit_cycle(&snap.pty_id);
        let result =
            self.deliver_pty_input_claimed(&snap.pty_id, &exit_command, true, claim.clone());
        if claim.provider_exited() {
            crate::service::agent_restart::timing::stage(
                &snap.tab_id,
                &snap.pty_id,
                "provider-exit",
            );
            return;
        }
        if let Err(error) = result {
            log::warn!(
                "[agent_restart] graceful exit send failed for tab={}: {}",
                snap.tab_id,
                error
            );
            return;
        }
        let _ = crate::service::pty_io::await_bare_shell_prompt(
            &self.screen_state,
            &snap.pty_id,
            grace,
        );
    }

    fn idle_restart_termination_allowed(&self, snap: &TabRestartSnapshot) -> bool {
        let supported = self
            .agent_registry
            .with_provider(&snap.provider, |provider| {
                provider.supports_idle_restart_termination()
            })
            .unwrap_or(false);
        if !supported {
            return false;
        }
        let Some(snapshot) = self.try_snapshot_with_reconciled_status() else {
            return false;
        };
        let Some(tab) = snapshot.tabs.get(&snap.tab_id) else {
            return false;
        };
        let verified = verified_restart_session(snap, tab);
        let idle = snapshot.tab_statuses.get(&snap.tab_id)
            == Some(&codeterm_types::pty::TabStatus::Idle)
            && tab
                .agent_state
                .as_ref()
                .is_none_or(|state| state.state == codeterm_types::pty::TimelineState::Idle);
        idle_restart_termination_allowed(
            supported,
            idle,
            verified,
            !tab.provider_signals.background.guarded_items().is_empty(),
        )
    }

    pub(super) fn tear_down_restart_pty(&self, snap: &TabRestartSnapshot) -> Result<(), String> {
        self.pty_ownership.release_pty(&snap.pty_id);
        let reaper = {
            let mut manager = self.pty.lock();
            match manager.begin_kill(&snap.pty_id) {
                Ok(reaper) => Some(reaper),
                Err(_) if manager.get_session(&snap.pty_id).is_none() => None,
                Err(error) => return Err(error),
            }
        };
        if let Some(reaper) = reaper {
            reaper.wait_for_termination_bounded(Duration::from_secs(5))?;
        }
        crate::service::agent_restart::timing::stage(&snap.tab_id, &snap.pty_id, "process-exit");
        self.signal_worker_interruption(
            &snap.tab_id,
            crate::service::worker_interruption::WorkerInterruptionEvent::ForcedContextReset,
        );
        self.screen_state.lock().remove(&snap.pty_id);
        crate::service::stream_preview::remove(&snap.pty_id);
        self.clear_tab_agent_identity_after_confirmed_exit(
            &snap.tab_id,
            "agent restart reaped the previous PTY",
        );
        {
            let mut mux = self.mux.write();
            let tab = mux
                .get_tab_mut(&snap.tab_id)
                .ok_or_else(|| format!("Tab {} disappeared during restart", snap.tab_id))?;
            tab.pty_id = None;
            crate::service::boot_stage::apply_boot_stage(
                tab,
                codeterm_types::pty::BootStage::PtyStarting,
                None,
            );
        }
        crate::service::boot_wait::notify_boot_changed();
        self.emit_state_changed();
        Ok(())
    }
}

fn idle_restart_termination_allowed(
    supported: bool,
    idle: bool,
    verified: bool,
    background_work: bool,
) -> bool {
    supported && idle && verified && !background_work
}

fn verified_restart_session(
    snap: &TabRestartSnapshot,
    tab: &crate::multiplexer::types::Tab,
) -> bool {
    tab.pty_id.as_deref() == Some(snap.pty_id.as_str())
        && tab.agent_type.as_deref() == Some(snap.provider.as_str())
        && snap.session_id.is_some()
        && tab.agent_session_id == snap.session_id
        && tab.session_binding_nonce.is_some()
        && tab.session_binding_nonce == tab.reported_session_binding_nonce
        && !tab.agent_booting
        && tab.pending_prompt.is_none()
}

#[cfg(test)]
mod tests;
