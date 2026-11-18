use super::{
    relaunch::{self, Relaunch},
    AgentRestartError, RestartAttemptError, RestartOutcome, TabRestartSnapshot,
    RESTART_READY_TIMEOUT_SECS,
};
use crate::multiplexer::types::Tab;
use crate::prelude::*;
use crate::service::agent_lifecycle::{
    worker_binding::RestartBindingAuthority, AccountRestartTarget,
};
use crate::service::boot_stage::BootStage;
use crate::service::provider_accounts::PtyInputRevision;
use crate::service::{ServiceContext, TabPtyClaim};
use codeterm_plugin::provider_account::with_context;
use std::time::{Duration, Instant};

fn validate_pty(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    target: &AccountRestartTarget,
) -> Result<(), AgentRestartError> {
    target
        .validate_target()
        .map_err(AgentRestartError::BadRequest)?;
    let turn_boundary = target.terminal_turn.as_deref().map_or_else(
        || svc.tab_is_idle(&snap.tab_id),
        |turn| {
            crate::service::provider_accounts::account_turn_boundary(
                svc,
                &snap.tab_id,
                &target.expected,
                turn,
            )
        },
    );
    if target.expected.unbound_login
        || target.expected.refresh_login
        || target.expected.runtime_epoch.is_some()
        || !turn_boundary
    {
        return Err(AgentRestartError::BadRequest(
            "Account restart requires an idle turn boundary".into(),
        ));
    }
    let mux = svc.mux.read();
    let tab = mux
        .get_tab(&snap.tab_id)
        .ok_or_else(|| AgentRestartError::NotFound(snap.tab_id.clone()))?;
    target
        .expected
        .validate(tab, None)
        .map_err(AgentRestartError::BadRequest)?;
    if tab.pty_id.as_deref() != Some(&snap.pty_id)
        || snap.provider != target.expected.provider
        || snap.session_id.as_deref() != Some(&target.expected.session_id)
    {
        return Err(AgentRestartError::BadRequest(
            "Account restart PTY generation changed".into(),
        ));
    }
    Ok(())
}

fn commit_pty(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    target: &AccountRestartTarget,
) -> Result<(), AgentRestartError> {
    if !svc.old_pty_was_torn_down(snap) {
        return Err(AgentRestartError::BadRequest(
            "Account restart requires confirmed PTY cleanup".into(),
        ));
    }
    let mut mux = svc.mux.write();
    let tab = mux
        .get_tab_mut(&snap.tab_id)
        .ok_or_else(|| AgentRestartError::NotFound(snap.tab_id.clone()))?;
    if tab.agent_type.is_some()
        || tab.agent_session_id.is_some()
        || tab.session_binding_nonce.is_some()
    {
        return Err(AgentRestartError::BadRequest(
            "Account restart identity was not retired".into(),
        ));
    }
    target
        .commit_retired_selection(tab)
        .map_err(AgentRestartError::BadRequest)
}

pub(super) struct PtyAccountSwitch<'a> {
    target: &'a AccountRestartTarget,
    target_launch: Relaunch,
    source: AccountRestartTarget,
    source_launch: Relaunch,
}

impl<'a> PtyAccountSwitch<'a> {
    pub(super) fn prepare(
        svc: &ServiceContext,
        snap: &TabRestartSnapshot,
        target: &'a AccountRestartTarget,
    ) -> Result<Self, AgentRestartError> {
        validate_pty(svc, snap, target)?;
        let source = AccountRestartTarget {
            expected: target.expected.clone(),
            account_id: target.expected.account_id.clone(),
            account_namespace: target.expected.account_namespace.clone(),
            invocation: crate::service::provider_accounts::for_tab(svc, &snap.tab_id)
                .map_err(AgentRestartError::BadRequest)?,
            terminal_turn: None,
        };
        source
            .validate_target()
            .map_err(AgentRestartError::BadRequest)?;
        let tab = svc
            .mux
            .read()
            .get_tab(&snap.tab_id)
            .cloned()
            .ok_or_else(|| AgentRestartError::NotFound(snap.tab_id.clone()))?;
        if let Some(problem) = tab
            .cwd
            .as_deref()
            .and_then(crate::service::spawn_cwd::cwd_problem)
        {
            return Err(AgentRestartError::BadRequest(format!(
                "Working directory {problem}"
            )));
        }
        let prepare = |account: &AccountRestartTarget, label: &str| {
            with_context(account.invocation.clone(), || {
                let exists =
                    svc.restart_session_exists(&tab, &snap.provider, snap.session_id.as_deref());
                if exists != Some(true) {
                    log::debug!(
                        "[account-restart] tab={} provider={} account={label} exact session preflight refused: exists={exists:?}",
                        snap.tab_id,
                        snap.provider,
                    );
                    return Err(AgentRestartError::BadRequest(format!(
                        "The exact conversation is unavailable on the {label} account"
                    )));
                }
                relaunch::build_account_relaunch(svc, snap, account).ok_or_else(|| {
                    AgentRestartError::BadRequest(format!(
                        "The harness cannot resume the conversation on the {label} account"
                    ))
                })
            })
        };
        let source_launch = prepare(&source, "original")?;
        let target_launch = prepare(target, "selected")?;
        Ok(Self {
            target,
            target_launch,
            source,
            source_launch,
        })
    }

    pub(super) fn execute(
        self,
        svc: &ServiceContext,
        snap: &TabRestartSnapshot,
        outcome: RestartOutcome,
        claim: TabPtyClaim,
        authority: Option<RestartBindingAuthority>,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        self.execute_with_launch(svc, snap, outcome, claim, authority, launch_resume)
    }

    fn execute_with_launch(
        self,
        svc: &ServiceContext,
        snap: &TabRestartSnapshot,
        mut outcome: RestartOutcome,
        claim: TabPtyClaim,
        authority: Option<RestartBindingAuthority>,
        mut launch: impl FnMut(
            &ServiceContext,
            &TabRestartSnapshot,
            Relaunch,
            &AccountRestartTarget,
        ) -> Option<String>,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        validate_pty(svc, snap, self.target)?;
        crate::service::orchestrator::supersede_tab_launch(&snap.tab_id);
        svc.tear_down_restart_pty(snap)
            .map_err(|reason| RestartAttemptError::Failed {
                outcome: outcome.clone(),
                reason,
                timed_out: false,
            })?;
        outcome.old_pty_torn_down = true;
        commit_pty(svc, snap, self.target)?;
        svc.stage_runtime_restart_binding(&snap.tab_id, authority.clone());
        let target_input = PtyInputRevision::capture(svc, &snap.tab_id);
        let target_nonce = launch(svc, snap, self.target_launch, self.target);
        drop(claim);
        let target_result = settle_resume(
            svc,
            snap,
            self.target,
            &outcome,
            target_nonce.as_deref(),
            authority.is_some(),
        );
        let failure = match target_result {
            Ok(ready) => return Ok(ready),
            Err(failure) => failure,
        };
        let mut rollback_started = false;
        let rollback = (|| {
            let claim = svc.pty_ownership.acquire(&snap.tab_id)?;
            let retirement = svc.reserve_pty_input_retirement(&claim)?;
            let failed_pty = failed_launch_pty(
                svc,
                snap,
                self.target,
                target_nonce.as_deref(),
                &target_input,
            )?;
            rollback_started = true;
            svc.clear_runtime_restart_binding(&snap.tab_id);
            crate::service::orchestrator::supersede_tab_launch(&snap.tab_id);
            let cleanup = if let Some(pty_id) = failed_pty {
                svc.tear_down_restart_pty(&TabRestartSnapshot {
                    pty_id,
                    ..snap.clone()
                })
            } else {
                Ok(())
            };
            svc.clear_tab_agent_identity_explicitly(&snap.tab_id, "account switch launch retired");
            restore_source_selection(svc, snap, self.target, &self.source)?;
            svc.schedule_session_persist_debounced();
            cleanup?;
            svc.stage_runtime_restart_binding(&snap.tab_id, authority.clone());
            drop(retirement);
            let source_nonce = launch(svc, snap, self.source_launch, &self.source);
            drop(claim);
            settle_resume(
                svc,
                snap,
                &self.source,
                &outcome,
                source_nonce.as_deref(),
                authority.is_some(),
            )
            .map_err(|failure| failure.reason)
        })();
        let mut reason = failure.reason;
        match rollback {
            Ok(restored) => {
                outcome = restored;
                outcome.source_account_restored = true;
                svc.schedule_session_persist_debounced();
            }
            Err(restore_error) => {
                if rollback_started {
                    svc.clear_runtime_restart_binding(&snap.tab_id);
                }
                reason = format!(
                    "{reason}. The original conversation could not be resumed: {restore_error}"
                );
            }
        }
        Err(RestartAttemptError::Failed {
            outcome,
            reason,
            timed_out: failure.timed_out,
        })
    }
}

fn launch_resume(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    prepared: Relaunch,
    account: &AccountRestartTarget,
) -> Option<String> {
    svc.set_and_lock_tab_agent_type_from(
        &snap.tab_id,
        snap.provider.clone(),
        crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
        None,
    );
    crate::service::orchestrator::launch_agent_in_background_with_invocation(
        svc.clone(),
        snap.tab_id.clone(),
        prepared.command,
        snap.provider.clone(),
        crate::service::launch_diagnostics::AgentLaunchOrigin::Resume,
        false,
        None,
        None,
        None,
        Default::default(),
        None,
        None,
        snap.session_id.clone(),
        Some(account.invocation.clone()),
    )
}

struct ResumeFailure {
    reason: String,
    timed_out: bool,
}

fn wait_for_resume(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    account: &AccountRestartTarget,
    nonce: Option<&str>,
) -> Result<String, ResumeFailure> {
    wait_for_resume_until(
        svc,
        snap,
        account,
        nonce,
        Instant::now() + Duration::from_secs(RESTART_READY_TIMEOUT_SECS),
    )
}

fn wait_for_resume_until(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    account: &AccountRestartTarget,
    nonce: Option<&str>,
    deadline: Instant,
) -> Result<String, ResumeFailure> {
    let nonce = nonce.ok_or_else(|| ResumeFailure {
        reason: "The harness did not start an exact-session launch".into(),
        timed_out: false,
    })?;
    let mut result = None;
    crate::service::boot_wait::wait_until(deadline, || {
        let mux = svc.mux.read();
        result = match mux.get_tab(&snap.tab_id) {
            Some(tab)
                if selected(tab, account)
                    && tab.session_binding_nonce.as_deref() == Some(nonce) =>
            {
                match tab.boot_stage {
                    Some(BootStage::SpawnFailed) => Some(Err(tab
                        .boot_failure_reason
                        .clone()
                        .unwrap_or_else(|| "The harness launch failed".into()))),
                    Some(BootStage::AgentReady) if tab.launch_session_authenticated() => {
                        match tab.pty_id.as_ref().filter(|id| *id != &snap.pty_id) {
                            Some(pty)
                                if tab.agent_type.as_deref() == Some(snap.provider.as_str())
                                    && tab.agent_session_id == snap.session_id =>
                            {
                                Some(Ok(pty.clone()))
                            }
                            _ => Some(Err("The harness opened a different conversation".into())),
                        }
                    }
                    _ => None,
                }
            }
            _ => Some(Err(
                "The account launch ended before the original conversation was confirmed".into(),
            )),
        };
        result.is_some()
    });
    result
        .ok_or_else(|| ResumeFailure {
            reason: "The harness did not confirm the original conversation before the account switch deadline".into(),
            timed_out: true,
        })?
        .map_err(|reason| ResumeFailure { reason, timed_out: false })
}

fn settle_resume(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    account: &AccountRestartTarget,
    outcome: &RestartOutcome,
    nonce: Option<&str>,
    require_attempt_carry: bool,
) -> Result<RestartOutcome, ResumeFailure> {
    wait_for_resume(svc, snap, account, nonce)?;
    let _claim = svc
        .pty_ownership
        .acquire(&snap.tab_id)
        .map_err(|reason| ResumeFailure {
            reason,
            timed_out: false,
        })?;
    let pty_id = wait_for_resume_until(svc, snap, account, nonce, Instant::now())?;
    let mut outcome = outcome.clone();
    outcome.agent_ready = true;
    outcome.new_pty_id = Some(pty_id);
    svc.settle_restored_binding(outcome, snap, require_attempt_carry)
        .map_err(|error| ResumeFailure {
            reason: match error {
                RestartAttemptError::Failed { reason, .. } => reason,
                other => format!("{other:?}"),
            },
            timed_out: false,
        })
}

fn selected(tab: &Tab, account: &AccountRestartTarget) -> bool {
    tab.restorable_launch.account_id == account.account_id
        && tab.restorable_launch.account_namespace == account.account_namespace
}

fn failed_launch_pty(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    account: &AccountRestartTarget,
    nonce: Option<&str>,
    target_input: &PtyInputRevision,
) -> Result<Option<String>, String> {
    if !target_input.is_current(svc, &snap.tab_id) {
        return Err("Newer input belongs to the selected account launch".into());
    }
    let mux = svc.mux.read();
    let tab = mux.get_tab(&snap.tab_id).ok_or("The account tab closed")?;
    let owns_launch = match nonce {
        Some(nonce) => {
            tab.session_binding_nonce.as_deref() == Some(nonce)
                || (tab.session_binding_nonce.is_none()
                    && tab.agent_type.is_none()
                    && tab
                        .prior_session_binding_nonces
                        .last()
                        .is_some_and(|prior| prior.nonce == nonce))
        }
        None => tab.session_binding_nonce.is_none() && tab.pty_id.is_none(),
    };
    if !selected(tab, account) || !owns_launch {
        return Err("The tab changed while the account switch was in progress".into());
    }
    if tab
        .agent_state
        .as_ref()
        .is_some_and(|state| state.state == codeterm_types::pty::TimelineState::Working)
    {
        return Err("A turn is active on the selected account launch".into());
    }
    Ok(tab.pty_id.clone())
}

fn restore_source_selection(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    target: &AccountRestartTarget,
    source: &AccountRestartTarget,
) -> Result<(), String> {
    let mut mux = svc.mux.write();
    let tab = mux
        .get_tab_mut(&snap.tab_id)
        .ok_or("The account tab closed")?;
    if !selected(tab, target) {
        return Err("The tab account changed during cleanup".into());
    }
    tab.restorable_launch.account_id = source.account_id.clone();
    tab.restorable_launch.account_namespace = source.account_namespace.clone();
    Ok(())
}

#[cfg(test)]
mod tests;
