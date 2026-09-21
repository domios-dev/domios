//! In-place recovery of a provider that exited to its tab's surviving shell:
//! the owning provider resumes the tab's own verified session on the same PTY.

use std::time::Duration;

use codeterm_types::pty::{AgentLiveness, BootStage};

use super::{
    build_relaunch_command, evidenced_transcript_block, AgentRestartError, RestartAttemptError,
    RestartOutcome, TabRestartSnapshot, RESTART_READY_TIMEOUT_SECS,
};
use crate::agent::provider_identity::provider_for_claimed_session;
use crate::multiplexer::types::{PriorBindingNonce, TabBacking};
use crate::service::ServiceContext;
use crate::session::lifecycle::{JournalRecord, LifecycleTransition, TabLifecycleJournal};

pub(crate) const EXIT_RECOVERY_NO_LIVE_SHELL: &str = "exit_recovery_no_live_shell";
pub(crate) const EXIT_RECOVERY_NO_VERIFIED_SESSION: &str = "exit_recovery_no_verified_session";
pub(crate) const EXIT_RECOVERY_SESSION_NOT_OWNED: &str = "exit_recovery_session_not_owned";

struct ExitedShell {
    pty_id: String,
    cwd: Option<String>,
    lineage: Vec<PriorBindingNonce>,
    role: Option<String>,
    role_profile: Option<String>,
}

fn refusal(code: &str, detail: String) -> AgentRestartError {
    AgentRestartError::BadRequest(format!("{code}: {detail}"))
}

/// The journal records `session_bound` and checkpoint authority only for
/// nonce-proven bindings, so every id returned was verified in this tab.
pub(super) fn verified_session_ids(records: &[JournalRecord], tab_id: &str) -> Vec<String> {
    records
        .iter()
        .filter(|record| record.tab_id() == tab_id)
        .filter_map(|record| match record {
            JournalRecord::Transition { entry } => match &entry.transition {
                LifecycleTransition::SessionBound { session_id, .. } => Some(session_id.clone()),
                _ => None,
            },
            JournalRecord::Checkpoint { checkpoint } => {
                Some(checkpoint.authority.session_id.clone())
            }
        })
        .collect()
}

/// The newest lineage entry names the session; the journal must prove it was
/// verified. A tab without lineage falls back to its newest journaled binding.
pub(super) fn select_recovery_session(
    lineage: &[PriorBindingNonce],
    verified: &[String],
) -> Result<String, AgentRestartError> {
    let Some(latest) = lineage.last() else {
        return verified.last().cloned().ok_or_else(|| {
            refusal(
                EXIT_RECOVERY_NO_VERIFIED_SESSION,
                "no agent in this Tab and no verified prior session to recover; spawn a fresh agent"
                    .to_string(),
            )
        });
    };
    let Some(session_id) = latest.session_id.as_deref() else {
        return Err(refusal(
            EXIT_RECOVERY_NO_VERIFIED_SESSION,
            "the exited launch never bound a session; spawn a fresh agent".to_string(),
        ));
    };
    if verified.iter().any(|verified| verified == session_id) {
        return Ok(session_id.to_string());
    }
    Err(refusal(
        EXIT_RECOVERY_NO_VERIFIED_SESSION,
        format!("session {session_id} was never nonce-verified in this Tab; spawn a fresh agent"),
    ))
}

pub(super) fn recovery_provider(
    registry: &crate::agent::registry::AgentRegistry,
    cwd: Option<&str>,
    session_id: &str,
) -> Result<String, AgentRestartError> {
    let cwd = cwd.ok_or_else(|| {
        refusal(
            EXIT_RECOVERY_SESSION_NOT_OWNED,
            format!("session {session_id} cannot be checked: the Tab has no working directory"),
        )
    })?;
    provider_for_claimed_session(registry, cwd, session_id).ok_or_else(|| {
        refusal(
            EXIT_RECOVERY_SESSION_NOT_OWNED,
            format!("no single provider owns session {session_id} in {cwd}; spawn a fresh agent"),
        )
    })
}

impl ServiceContext {
    /// `Some` when the tab's agent is observed absent; its stale identity is
    /// first retired into lineage by the ordinary lifecycle teardown.
    pub(super) fn absent_restart_target(
        &self,
        tab_id: &str,
    ) -> Result<Option<String>, AgentRestartError> {
        let resolved = self
            .mux
            .read()
            .resolve_tab_id(tab_id)
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;
        if crate::agent::live_state::observe_tab_liveness(self, &resolved) != AgentLiveness::Absent
        {
            return Ok(None);
        }
        self.reconcile_tab_restored_without_agent(&resolved);
        Ok(Some(resolved))
    }

    pub(super) fn recover_exited_provider(
        &self,
        tab_id: &str,
        force: bool,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        let shell = self.exited_shell(tab_id)?;
        let verified = verified_session_ids(&TabLifecycleJournal::shared().load().records, tab_id);
        let session_id = select_recovery_session(&shell.lineage, &verified)?;
        let provider = recovery_provider(&self.agent_registry, shell.cwd.as_deref(), &session_id)?;
        let auto_approve = crate::service::permission_policy::configured_permission_bypass(
            &self.agent_registry,
            &provider,
        )
        .map_err(AgentRestartError::BadRequest)?
        .is_some();
        let snap = TabRestartSnapshot {
            tab_id: tab_id.to_string(),
            pty_id: shell.pty_id,
            provider,
            session_id: Some(session_id),
            cwd: shell.cwd,
            auto_approve,
            role: shell.role,
            role_profile: shell.role_profile,
        };
        if let Some(reason) = evidenced_transcript_block(&snap) {
            return Err(AgentRestartError::BadRequest(reason).into());
        }

        self.check_and_record_restart(&snap.tab_id)?;
        self.record_lifecycle(
            &snap.tab_id,
            crate::session::lifecycle::LifecycleTransition::RestartRequested,
        );
        let mut outcome = RestartOutcome::accepted(&snap, false, force);
        outcome.recovered_in_place = true;
        let Some(launch_cmd) = build_relaunch_command(self, &snap, true, None)
            .map(|command| crate::agent::stub_launch::apply(&snap.provider, command))
        else {
            return Err(RestartAttemptError::Failed {
                reason: format!("provider {} returned no resume command", snap.provider),
                outcome,
                timed_out: false,
            });
        };
        if let Err(reason) = self.retire_exited_shell_stage(&snap) {
            return Err(RestartAttemptError::Failed {
                outcome,
                reason,
                timed_out: false,
            });
        }

        self.set_and_lock_tab_agent_type_from(
            &snap.tab_id,
            snap.provider.clone(),
            crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
            None,
        );
        crate::service::orchestrator::launch_agent_in_background(
            self.clone(),
            snap.tab_id.clone(),
            launch_cmd,
            snap.provider.clone(),
            crate::service::launch_diagnostics::AgentLaunchOrigin::ExitRecovery,
            None,
            None,
            None,
            None,
        );

        let ready = self.finish_observed_restart(
            outcome,
            None,
            Duration::from_secs(RESTART_READY_TIMEOUT_SECS),
        )?;
        self.settle_restored_binding(ready, &snap)
    }

    fn exited_shell(&self, tab_id: &str) -> Result<ExitedShell, AgentRestartError> {
        let (pty_id, cwd, lineage, role, role_profile) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;
            if matches!(tab.backing, TabBacking::ChatBackend { .. }) {
                return Err(AgentRestartError::BadRequest(
                    "agent session is closed; a chat-backend Tab has no provider shell to recover"
                        .to_string(),
                ));
            }
            (
                tab.pty_id.clone(),
                tab.cwd.clone(),
                tab.prior_session_binding_nonces.clone(),
                tab.role.map(|role| format!("{role:?}")),
                tab.role_profile.clone(),
            )
        };
        let pty_id = pty_id
            .filter(|pty_id| self.pty.lock().get_session(pty_id).is_some())
            .ok_or_else(|| {
                refusal(
                    EXIT_RECOVERY_NO_LIVE_SHELL,
                    "the agent process is absent and the Tab has no live shell to relaunch it in"
                        .to_string(),
                )
            })?;
        Ok(ExitedShell {
            pty_id,
            cwd,
            lineage,
            role,
            role_profile,
        })
    }

    /// The exited shell's own stage must never satisfy the in-place readiness wait.
    fn retire_exited_shell_stage(&self, snap: &TabRestartSnapshot) -> Result<(), String> {
        {
            let mut mux = self.mux.write();
            let tab = mux
                .get_tab_mut(&snap.tab_id)
                .ok_or_else(|| format!("Tab {} disappeared during recovery", snap.tab_id))?;
            if tab.pty_id.as_deref() != Some(snap.pty_id.as_str()) {
                return Err(format!(
                    "Tab {} changed its shell during recovery",
                    snap.tab_id
                ));
            }
            tab.auto_approve = snap.auto_approve;
            crate::service::boot_stage::apply_boot_stage(tab, BootStage::PtyStarting, None);
        }
        crate::service::boot_wait::notify_boot_changed();
        self.emit_state_changed();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
