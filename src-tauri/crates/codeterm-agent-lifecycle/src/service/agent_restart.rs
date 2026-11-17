//! Agent restart: graceful /exit + auto-resume, force-kill PTY + relaunch, or
//! in-place recovery of a provider that exited to its shell.
//!
//! Wraps the manual /exit + resume dance documented in AGENT.md as a single
//! atomic primitive callable from the CLI (`domios agent restart`) and from
//! the self-update flow (`feat/auto-update`) which restarts every agent tab
//! around binary swap.

use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_types::pty::AgentLiveness;

use super::ServiceContext;
use crate::agent::provider::{AgentLaunchParams, ProviderTransport, StructuredPromptServer};
use crate::prelude::*;
use crate::service::agent_lifecycle::restart_carries_task_attempt;
#[cfg(test)]
use crate::service::agent_lifecycle::{
    command_prompt_owner_matches, new_restart_tracker, screen_shows_agent_tui,
};
pub use crate::service::agent_lifecycle::{
    AgentRestartError, ControlChange, ModelSwitchConfirmation, ModelSwitchError,
    ModelSwitchEvidence, ModelSwitchOutcome, RestartAttemptError, RestartOptions, RestartOutcome,
    RestartTracker, RestartTrackerState, TabRestartSnapshot, DEFAULT_GRACE_SECS,
    RESTART_RATE_LIMIT_SECS, RESTART_READY_TIMEOUT_SECS,
};
#[cfg(test)]
use parking_lot::Mutex;

pub struct RestartClaim {
    tracker: RestartTracker,
    tab_id: String,
}

impl RestartClaim {
    fn release_cooldown(&self) {
        self.tracker.lock().last_restart.remove(&self.tab_id);
    }
}

impl Drop for RestartClaim {
    fn drop(&mut self) {
        let mut tracker = self.tracker.lock();
        tracker.command_ptys.remove(&self.tab_id);
        tracker.in_progress.remove(&self.tab_id);
    }
}

pub trait ProviderSwitchClaimExt {
    fn claim_provider_switch(&self, tab_id: &str) -> Result<RestartClaim, AgentRestartError>;
}

impl ProviderSwitchClaimExt for ServiceContext {
    fn claim_provider_switch(&self, tab_id: &str) -> Result<RestartClaim, AgentRestartError> {
        // An explicit handoff shares the lifecycle lock without throttling the owner's choice.
        self.claim_restart(tab_id, true)
    }
}

pub(super) fn claim_restart_attempt(
    tracker: &RestartTracker,
    tab_id: &str,
    automatic: bool,
) -> Result<RestartClaim, AgentRestartError> {
    let mut state = tracker.lock();
    let now = Instant::now();
    if state.in_progress.contains(tab_id) {
        return Err(AgentRestartError::BadRequest(
            "an agent restart or automatic recovery is already in progress".to_string(),
        ));
    }
    if !automatic {
        if let Some(last) = state.last_restart.get(tab_id) {
            let elapsed = now.duration_since(*last);
            let limit = Duration::from_secs(RESTART_RATE_LIMIT_SECS);
            if elapsed < limit {
                return Err(AgentRestartError::RateLimited {
                    retry_after_secs: (limit - elapsed).as_secs().max(1),
                });
            }
        }
    }
    state.last_restart.insert(tab_id.to_string(), now);
    state.in_progress.insert(tab_id.to_string());
    Ok(RestartClaim {
        tracker: tracker.clone(),
        tab_id: tab_id.to_string(),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RestartMode {
    Standard,
    SkipExit,
    Fresh,
}

pub trait AgentRestartExt {
    fn tab_is_idle(&self, tab_id: &str) -> bool;
    fn agent_restart(
        &self,
        tab_id: &str,
        force: bool,
        grace_secs: u64,
    ) -> Result<RestartOutcome, AgentRestartError>;
    fn agent_restart_observed(
        &self,
        tab_id: &str,
        force: bool,
        grace_secs: u64,
    ) -> Result<RestartOutcome, RestartAttemptError>;
    fn prepare_provider_change(
        &self,
        snap: &mut TabRestartSnapshot,
        target_provider: &str,
        handoff_history: bool,
    ) -> Result<(), AgentRestartError>;
    fn restart_live_agent(
        &self,
        snap: TabRestartSnapshot,
        force: bool,
        grace_secs: u64,
        model_switch: Option<&model_switch::ModelSwitchLaunch>,
        mode: RestartMode,
    ) -> Result<RestartOutcome, RestartAttemptError>;
    fn restart_live_agent_with_account(
        &self,
        snap: TabRestartSnapshot,
        force: bool,
        grace_secs: u64,
        model_switch: Option<&model_switch::ModelSwitchLaunch>,
        mode: RestartMode,
        account: Option<&crate::service::agent_lifecycle::AccountRestartTarget>,
    ) -> Result<RestartOutcome, RestartAttemptError>;
    fn settle_restored_binding(
        &self,
        outcome: RestartOutcome,
        snap: &TabRestartSnapshot,
        require_attempt_carry: bool,
    ) -> Result<RestartOutcome, RestartAttemptError>;
    fn restart_general_agent_if_target(
        &self,
        tab_id: &str,
        force: bool,
    ) -> Result<Option<Result<RestartOutcome, RestartAttemptError>>, AgentRestartError>;
    fn general_agent_restart_snapshot(&self, tab_id: &str) -> Option<TabRestartSnapshot>;
    fn snapshot_tab_for_restart(
        &self,
        tab_id: &str,
    ) -> Result<TabRestartSnapshot, AgentRestartError>;
    fn require_tab_restart_liveness(&self, tab_id: &str) -> Result<(), AgentRestartError>;
    fn lazy_first_turn_restart_ready(&self, tab_id: &str) -> bool;
    fn require_background_work_safe(
        &self,
        tab_id: &str,
        force: bool,
    ) -> Result<(), AgentRestartError>;
    fn claim_restart(
        &self,
        tab_id: &str,
        automatic: bool,
    ) -> Result<RestartClaim, AgentRestartError>;
    fn session_file_present(&self, snap: &TabRestartSnapshot) -> bool;
    fn finish_observed_restart(
        &self,
        outcome: RestartOutcome,
        replaced_pty_id: Option<&str>,
        timeout: Duration,
    ) -> Result<RestartOutcome, RestartAttemptError>;
    fn log_restart_timeline(&self, result: &str, outcome: &RestartOutcome);
    fn old_pty_was_torn_down(&self, snap: &TabRestartSnapshot) -> bool;
}

impl AgentRestartExt for ServiceContext {
    fn tab_is_idle(&self, tab_id: &str) -> bool {
        let Some(snapshot) = self.try_snapshot_with_reconciled_status() else {
            return false;
        };
        snapshot.tab_statuses.get(tab_id) == Some(&codeterm_types::pty::TabStatus::Idle)
    }

    fn agent_restart(
        &self,
        tab_id: &str,
        force: bool,
        grace_secs: u64,
    ) -> Result<RestartOutcome, AgentRestartError> {
        self.agent_restart_observed(tab_id, force, grace_secs)
            .map_err(|error| match error {
                RestartAttemptError::Request(error) => error,
                RestartAttemptError::Failed { reason, .. } => AgentRestartError::Internal(reason),
            })
    }

    fn agent_restart_observed(
        &self,
        tab_id: &str,
        force: bool,
        grace_secs: u64,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        self.agent_restart_observed_with_options(
            tab_id,
            RestartOptions {
                force,
                grace_secs,
                ..Default::default()
            },
        )
    }

    fn prepare_provider_change(
        &self,
        snap: &mut TabRestartSnapshot,
        target_provider: &str,
        handoff_history: bool,
    ) -> Result<(), AgentRestartError> {
        self.require_restart_provider_handoff(&snap.provider, target_provider)?;
        let transport = self
            .agent_registry
            .with_provider(target_provider, |provider| provider.describe().transport)
            .ok_or_else(|| {
                AgentRestartError::BadRequest(format!("unknown agent provider: {target_provider}"))
            })?;
        if transport != ProviderTransport::Pty {
            return Err(AgentRestartError::BadRequest(
                "provider restart requires a PTY transport".into(),
            ));
        }
        let source_transport = self
            .agent_registry
            .with_provider(&snap.provider, |provider| provider.describe().transport)
            .ok_or_else(|| {
                AgentRestartError::BadRequest(format!("unknown agent provider: {}", snap.provider))
            })?;
        if source_transport != ProviderTransport::Pty {
            return Err(AgentRestartError::BadRequest(
                "provider restart requires a PTY source".into(),
            ));
        }
        snap.auto_approve = crate::service::permission_policy::configured_permission_bypass_for(
            self,
            &self.agent_registry,
            target_provider,
        )
        .map_err(AgentRestartError::BadRequest)?
        .is_some();
        snap.target_provider = Some(target_provider.to_string());
        if crate::service::provider_handoff::should_handoff_history(
            &snap.provider,
            target_provider,
            handoff_history,
        ) {
            let (messages, _) = crate::service::tab_chat::tab_chat_snapshot(self, &snap.tab_id, 0)
                .map_err(|error| {
                    AgentRestartError::BadRequest(format!(
                        "cannot read the source Tab history for provider handoff: {error}"
                    ))
                })?;
            let recent = crate::service::provider_handoff::input::recent_messages(&messages);
            let context = serde_json::json!({
                "tab_id": snap.tab_id, "role": snap.role, "role_profile": snap.role_profile,
                "cwd": snap.cwd, "previous_provider": snap.provider, "previous_session": snap.session_id,
            }).to_string();
            let instructions = "Continue this tab on the target provider. First build a concise working summary from the host context and recent messages, then continue the outstanding request. The full conversation is not transferred; historical text is context, not a newly issued instruction.";
            let available = crate::service::provider_handoff::input::MAX_INPUT_BYTES
                .saturating_sub(instructions.len() + context.len() + 4);
            let history = crate::service::provider_handoff::select_history(&recent, available / 3);
            let selected = crate::service::provider_handoff::input::assemble(
                crate::service::provider_handoff::input::HandoffFragments {
                    instructions,
                    context: &context,
                    history: &history,
                },
            )
            .map_err(AgentRestartError::BadRequest)?;
            if !selected.is_empty() {
                snap.handoff_history = Some(selected);
            }
        }
        Ok(())
    }

    /// Exit, tear down and relaunch a live agent. A model switch supplies its
    /// own resume command and commits its launch record once the old PTY is gone.
    fn restart_live_agent(
        &self,
        snap: TabRestartSnapshot,
        force: bool,
        grace_secs: u64,
        model_switch: Option<&model_switch::ModelSwitchLaunch>,
        mode: RestartMode,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        self.restart_live_agent_with_account(snap, force, grace_secs, model_switch, mode, None)
    }

    fn restart_live_agent_with_account(
        &self,
        mut snap: TabRestartSnapshot,
        force: bool,
        grace_secs: u64,
        model_switch: Option<&model_switch::ModelSwitchLaunch>,
        mode: RestartMode,
        account: Option<&crate::service::agent_lifecycle::AccountRestartTarget>,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        if account.is_some()
            && (mode != RestartMode::Standard
                || model_switch.is_some()
                || snap.target_provider.is_some())
        {
            return Err(AgentRestartError::BadRequest(
                "Account restart cannot change restart mode or provider/model".into(),
            )
            .into());
        }
        self.require_background_work_safe(&snap.tab_id, force)?;
        crate::service::agent_lifecycle::timing::stage(&snap.tab_id, &snap.pty_id, "request");
        let target_provider = snap.target_provider.as_deref().unwrap_or(&snap.provider);
        let provider_changed = target_provider != snap.provider;
        if mode != RestartMode::Fresh && !provider_changed {
            self.require_exact_session_restart(&snap)?;
        }
        if !provider_changed && mode != RestartMode::Fresh {
            if let Some(reason) = evidenced_transcript_block(&snap) {
                return Err(AgentRestartError::BadRequest(reason).into());
            }
        }

        let restart_claim = self.claim_restart(&snap.tab_id, false)?;
        if mode == RestartMode::Fresh {
            self.require_fresh_restart_allowed(&snap, force)?;
        }
        let pty_claim = self
            .pty_ownership
            .acquire(&snap.tab_id)
            .map_err(AgentRestartError::BadRequest)?;
        {
            let mux = self.mux.read();
            let tab = mux.get_tab(&snap.tab_id).ok_or_else(|| {
                AgentRestartError::NotFound(format!("Tab {} not found", snap.tab_id))
            })?;
            snap.pty_id = tab.pty_id.clone().unwrap_or_default();
            if mode == RestartMode::Fresh {
                snap.session_id = tab.agent_session_id.clone();
            }
        }
        if let Some(account) = account {
            account::validate_pty(self, &snap, account)?;
        }
        if mode == RestartMode::Fresh {
            self.require_fresh_restart_allowed(&snap, force)?;
        }
        crate::service::task_files::release_tab(self, &snap.tab_id);
        self.record_lifecycle(
            &snap.tab_id,
            crate::session::lifecycle::LifecycleTransition::RestartRequested,
        );

        let session_was_stale = mode == RestartMode::Fresh
            || (!provider_changed
                && snap.session_id.is_some()
                && !self.session_file_present(&snap));
        if mode == RestartMode::SkipExit && snap.session_id.is_some() && session_was_stale {
            return Err(AgentRestartError::BadRequest(
                "the saved recovery session is unavailable; refusing a fresh conversation".into(),
            )
            .into());
        }
        let mut outcome = RestartOutcome::accepted(&snap, session_was_stale, force);
        if mode == RestartMode::Fresh {
            outcome.session_id = None;
        }
        let session_present = !provider_changed && !session_was_stale && snap.session_id.is_some();
        if account.is_some() && !session_present {
            return Err(AgentRestartError::BadRequest(
                "Account restart session is unavailable".into(),
            )
            .into());
        }
        let build = || match model_switch {
            Some(launch)
                if session_present || (snap.session_id.is_none() && launch.is_sessionless()) =>
            {
                launch
                    .command(self, &snap)
                    .map(|(command, task)| (command.with_stub(&snap.provider), task))
            }
            Some(_) => None,
            None => relaunch::build_relaunch(self, &snap, session_present)
                .map(|relaunch| (relaunch.command, relaunch.task)),
        };
        let relaunch = match account {
            Some(account) => {
                codeterm_plugin::provider_account::with_context(account.invocation.clone(), || {
                    relaunch::build_account_relaunch(self, &snap, account)
                        .map(|relaunch| (relaunch.command, relaunch.task))
                })
            }
            None => build(),
        };
        let Some((launch_cmd, launch_task)) = relaunch else {
            crate::service::task_files::release_tab(self, &snap.tab_id);
            let reason = if model_switch.is_some() && !session_present {
                "the Tab's session is no longer resumable, so its model cannot be switched in place"
                    .to_string()
            } else {
                format!("provider {target_provider} returned no launch command")
            };
            return Err(RestartAttemptError::Failed {
                outcome,
                reason,
                timed_out: false,
            });
        };

        let launch_task = if account.is_some() { None } else { launch_task };
        let restart_binding = if session_present || provider_changed {
            let (nonce, task_bound) = {
                let mux = self.mux.read();
                let tab = mux.get_tab(&snap.tab_id).ok_or_else(|| {
                    AgentRestartError::NotFound(format!("Tab {} not found", snap.tab_id))
                })?;
                (
                    tab.session_binding_nonce.clone(),
                    restart_carries_task_attempt(tab.task_binding.as_ref()),
                )
            };
            let authority = nonce.as_deref().and_then(|nonce| {
                let result = if provider_changed {
                    self.authorize_provider_switch_binding(
                        &snap.tab_id,
                        &snap.provider,
                        target_provider,
                        nonce,
                    )
                } else if model_switch.is_some() {
                    self.authorize_model_switch_restart_binding(&snap.tab_id, &snap.provider, nonce)
                } else {
                    self.authorize_runtime_restart_binding(
                        &snap.tab_id,
                        &snap.provider,
                        nonce,
                        false,
                    )
                };
                result.ok().flatten()
            });
            if task_bound && authority.is_none() {
                return Err(AgentRestartError::BadRequest(
                    "task attempt could not be authorized for restart carry".into(),
                )
                .into());
            }
            Some(authority)
        } else {
            None
        };

        self.validate_completed_restart_binding(
            &snap.tab_id,
            restart_binding.as_ref().and_then(Option::as_ref),
        )
        .map_err(AgentRestartError::BadRequest)?;
        if mode != RestartMode::SkipExit
            && mode != RestartMode::Fresh
            && !account.is_some_and(|target| target.terminal_turn.is_some())
        {
            self.require_background_work_safe(&snap.tab_id, force)?;
            crate::service::agent_lifecycle::timing::stage(
                &snap.tab_id,
                &snap.pty_id,
                "exit-request",
            );
            self.request_graceful_exit(&snap, force, Duration::from_secs(grace_secs.max(1)));
        }
        self.require_background_work_safe(&snap.tab_id, force)?;
        if mode == RestartMode::Fresh {
            self.require_fresh_restart_allowed(&snap, force)?;
        } else if !provider_changed {
            self.require_exact_session_restart(&snap)?;
        }
        if force {
            super::background_work::force_notice(
                self,
                &snap.tab_id,
                &super::background_work::running(self, &snap.tab_id),
            );
        }
        if mode == RestartMode::Fresh {
            self.end_held_user_sends_for_fresh_restart(&snap.tab_id)
                .map_err(AgentRestartError::Internal)?;
        }
        if let Some(account) = account {
            account::validate_before_teardown(self, &snap, account)?;
        }
        let teardown = || {
            crate::service::orchestrator::supersede_tab_launch(&snap.tab_id);
            self.tear_down_restart_pty(&snap)
        };
        let teardown_result =
            if let Some(launch) = model_switch.filter(|launch| launch.is_sessionless()) {
                launch.teardown_sessionless_switch(self, &snap, teardown, || {
                    launch.commit(self, &snap.tab_id)
                })
            } else {
                teardown().map_err(model_switch::SessionlessTeardownError::Teardown)
            };
        if let Err(error) = teardown_result {
            let reason = match error {
                model_switch::SessionlessTeardownError::Admission(error) => {
                    return Err(error.into())
                }
                model_switch::SessionlessTeardownError::Teardown(reason) => reason,
            };
            crate::service::task_files::release_tab(self, &snap.tab_id);
            return Err(RestartAttemptError::Failed {
                outcome,
                reason,
                timed_out: false,
            });
        }
        outcome.old_pty_torn_down = true;
        if let Some(account) = account {
            account::commit_pty(self, &snap, account)?;
        }
        if mode == RestartMode::Fresh {
            self.commit_fresh_restart(&snap)?;
        }
        if provider_changed {
            let reset = {
                let mut mux = self.mux.write();
                mux.get_tab_mut(&snap.tab_id).map(|tab| {
                    crate::service::provider_handoff::reset_tab_identity_for_provider_change(
                        tab,
                        snap.auto_approve,
                    );
                })
            };
            if reset.is_none() {
                crate::service::task_files::release_tab(self, &snap.tab_id);
                return Err(RestartAttemptError::Failed {
                    outcome,
                    reason: format!("Tab {} disappeared during provider change", snap.tab_id),
                    timed_out: false,
                });
            }
            self.schedule_session_persist_debounced();
        }
        if let Some(launch) = model_switch.filter(|launch| !launch.is_sessionless()) {
            launch.commit(self, &snap.tab_id);
        }

        let attempt_carry_required = restart_binding.as_ref().is_some_and(Option::is_some);
        self.stage_runtime_restart_binding(
            &snap.tab_id,
            restart_binding
                .as_ref()
                .and_then(|authority| authority.clone()),
        );
        self.set_and_lock_tab_agent_type_from(
            &snap.tab_id,
            target_provider.to_string(),
            crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
            None,
        );
        crate::service::agent_lifecycle::timing::stage(
            &snap.tab_id,
            &snap.pty_id,
            "relaunch-request",
        );
        crate::service::orchestrator::launch_agent_in_background_with_invocation(
            self.clone(),
            snap.tab_id.clone(),
            launch_cmd,
            target_provider.to_string(),
            if session_present {
                crate::service::launch_diagnostics::AgentLaunchOrigin::Resume
            } else {
                crate::service::launch_diagnostics::AgentLaunchOrigin::Restart
            },
            false,
            None,
            launch_task,
            None,
            Default::default(),
            None,
            None,
            if session_present {
                snap.session_id.clone()
            } else {
                None
            },
            account.map(|target| target.invocation.clone()),
        );
        drop(pty_claim);
        drop(restart_claim);

        let ready = self.finish_observed_restart(
            outcome,
            Some(snap.pty_id.as_str()),
            Duration::from_secs(RESTART_READY_TIMEOUT_SECS),
        );
        let ready = match ready {
            Ok(ready) => ready,
            Err(error) => {
                self.clear_runtime_restart_binding(&snap.tab_id);
                crate::service::task_files::release_tab(self, &snap.tab_id);
                return Err(error);
            }
        };
        crate::service::agent_lifecycle::timing::stage(
            &snap.tab_id,
            ready.new_pty_id.as_deref().unwrap_or("-"),
            "ready",
        );
        let settled = self.settle_restored_binding(ready, &snap, attempt_carry_required);
        if settled.is_err() {
            self.clear_runtime_restart_binding(&snap.tab_id);
        }
        settled
    }

    /// Rebind callers before readiness is reported: a tab that cannot regain
    /// its exact binding must not be handed back as continuable.
    fn settle_restored_binding(
        &self,
        mut outcome: RestartOutcome,
        snap: &TabRestartSnapshot,
        require_attempt_carry: bool,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        // Only a resumed session has an id the tab must still be holding; a
        // fresh launch legitimately comes back on a new one.
        let expected = (outcome.session_action() == "resumed")
            .then_some(snap.session_id.as_deref())
            .flatten();
        if require_attempt_carry {
            let (thread, nonce) = self
                .mux
                .read()
                .get_tab(&snap.tab_id)
                .and_then(|tab| {
                    Some((
                        tab.agent_session_id.clone()?,
                        tab.session_binding_nonce.clone()?,
                    ))
                })
                .ok_or_else(|| RestartAttemptError::Failed {
                    outcome: outcome.clone(),
                    reason: "restarted task worker has no current session binding".into(),
                    timed_out: false,
                })?;
            match self.finalize_authorized_runtime_restart_binding(&snap.tab_id, &thread, &nonce) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(RestartAttemptError::Failed {
                        outcome,
                        reason: "restarted task worker attempt was not carried".into(),
                        timed_out: false,
                    });
                }
                Err(error) => {
                    return Err(RestartAttemptError::Failed {
                        outcome,
                        reason: error.reason,
                        timed_out: false,
                    });
                }
            }
        }
        match self.restore_caller_binding(&snap.tab_id, expected) {
            Ok(restored) => {
                outcome.binding_restored = true;
                if !restored.is_empty() {
                    log::info!(
                        "[agent_restart] tab={} rebound owner={:?} planner={:?} tasks={}",
                        snap.tab_id,
                        restored.owned_plan_ids,
                        restored.planner_plan_ids,
                        restored.task_bindings.len(),
                    );
                }
                Ok(outcome)
            }
            Err(error) => {
                crate::service::task_files::release_tab(self, &snap.tab_id);
                Err(RestartAttemptError::Failed {
                    outcome,
                    reason: error.to_string(),
                    timed_out: false,
                })
            }
        }
    }

    fn restart_general_agent_if_target(
        &self,
        tab_id: &str,
        force: bool,
    ) -> Result<Option<Result<RestartOutcome, RestartAttemptError>>, AgentRestartError> {
        let Some(mut target) = self.general_agent_restart_snapshot(tab_id) else {
            return Ok(None);
        };
        self.require_background_work_safe(&target.tab_id, force)?;
        let unconfirmed = self.mux.read().get_tab(&target.tab_id).is_some_and(|tab| {
            tab.boot_stage == Some(crate::service::boot_stage::BootStage::LaunchBindingUnconfirmed)
                && !tab.agent_booting
        });
        if unconfirmed {
            if let Some(recovery) = self.failed_launch_restart_snapshot(&target.tab_id)? {
                target = recovery;
            }
        }
        if self.is_unverified_launch_shell(&target) {
            target.session_id = None;
        }
        if let Some(reason) = evidenced_transcript_block(&target) {
            return Err(AgentRestartError::BadRequest(reason));
        }
        let restart_claim = self.claim_restart(&target.tab_id, false)?;

        let claimed =
            crate::service::general_agent::claim_general_agent_tab_retry(&target.tab_id, &self.mux)
                .map_err(AgentRestartError::BadRequest)?;
        if !claimed {
            return Err(AgentRestartError::BadRequest(
                "General Agent launch already in progress".to_string(),
            ));
        }
        if unconfirmed && target.session_id.is_some() {
            self.set_agent_session_id(&target.tab_id, target.session_id.clone());
        }
        crate::service::task_files::release_tab(self, &target.tab_id);

        let session_was_stale = target.session_id.is_some() && !self.session_file_present(&target);
        let outcome = RestartOutcome::accepted(&target, session_was_stale, force);
        let mux = self.mux.clone();
        let svc = Arc::new(self.clone());
        let pty = self.pty.clone();
        let screen_state = self.screen_state.clone();
        let config = self.ext::<crate::config::ConfigState>().clone();
        std::thread::spawn(move || {
            crate::service::general_agent::restart_general_agent_with_force(
                &svc,
                &mux,
                &pty,
                &screen_state,
                &config,
                force,
            );
        });
        drop(restart_claim);

        log::info!(
            "[agent_restart] Tab {} recovered via General Agent relaunch",
            target.tab_id
        );
        let result = self.finish_observed_restart(
            outcome,
            Some(target.pty_id.as_str()),
            Duration::from_secs(RESTART_READY_TIMEOUT_SECS),
        );
        let old_pty_torn_down = self.old_pty_was_torn_down(&target);
        Ok(Some(match result {
            Ok(mut ready) => {
                ready.old_pty_torn_down = old_pty_torn_down;
                Ok(ready)
            }
            Err(RestartAttemptError::Failed {
                mut outcome,
                reason,
                timed_out,
            }) => {
                crate::service::task_files::release_tab(self, &target.tab_id);
                outcome.old_pty_torn_down = old_pty_torn_down;
                Err(RestartAttemptError::Failed {
                    outcome,
                    reason,
                    timed_out,
                })
            }
            Err(error) => Err(error),
        }))
    }

    fn general_agent_restart_snapshot(&self, tab_id: &str) -> Option<TabRestartSnapshot> {
        let m = self.mux.read();
        let general = m.resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)?;
        if m.resolve_tab_id(tab_id)? != general {
            return None;
        }
        let tab = m.get_tab(&general)?;
        if !tab.protected {
            return None;
        }
        let provider = tab.agent_type.clone().or_else(|| {
            Some(
                self.ext::<crate::config::ConfigState>()
                    .lock()
                    .general_agent
                    .agent_provider
                    .clone(),
            )
        })?;
        Some(TabRestartSnapshot {
            tab_id: general,
            pty_id: tab.pty_id.clone().unwrap_or_default(),
            provider,
            target_provider: None,
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            auto_approve: false,
            role: tab.role.map(|role| format!("{role:?}")),
            role_profile: tab.role_profile.clone(),
            handoff_history: None,
        })
    }

    fn snapshot_tab_for_restart(
        &self,
        tab_id: &str,
    ) -> Result<TabRestartSnapshot, AgentRestartError> {
        self.require_tab_restart_liveness(tab_id)?;

        self.snapshot_tab_restart_state(tab_id)
    }

    fn require_tab_restart_liveness(&self, tab_id: &str) -> Result<(), AgentRestartError> {
        let resolved = self
            .mux
            .read()
            .resolve_tab_id(tab_id)
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;
        let liveness = crate::agent::live_state::observe_tab_liveness(self, &resolved);
        let first_turn_pty = self
            .mux
            .read()
            .get_tab(&resolved)
            .and_then(|tab| tab.pty_id.clone());
        if liveness == AgentLiveness::Unobserved
            && self.lazy_first_turn_restart_ready(&resolved)
            && first_turn_pty.is_some_and(|id| self.pty.lock().get_session(&id).is_some())
        {
            return Ok(());
        }
        if liveness == AgentLiveness::Absent {
            self.reconcile_tab_restored_without_agent(&resolved);
        }
        require_observed_live_agent(liveness)
    }

    fn lazy_first_turn_restart_ready(&self, tab_id: &str) -> bool {
        let tab = self.mux.read().get_tab(tab_id).cloned();
        tab.is_some_and(|tab| {
            crate::service::boot_stage::first_turn_input_is_ready(&tab)
                && tab.agent_type.as_deref().is_some_and(|provider| {
                    self.agent_registry
                        .with_provider(provider, |p| p.session_starts_on_first_user_turn())
                        == Some(true)
                })
        })
    }

    fn require_background_work_safe(
        &self,
        tab_id: &str,
        force: bool,
    ) -> Result<(), AgentRestartError> {
        let items = super::background_work::running(self, tab_id);
        super::background_work::restart_guard(items, force)
    }

    fn claim_restart(
        &self,
        tab_id: &str,
        automatic: bool,
    ) -> Result<RestartClaim, AgentRestartError> {
        claim_restart_attempt(
            self.ext::<crate::service::agent_restart::RestartTracker>(),
            tab_id,
            automatic,
        )
    }

    fn session_file_present(&self, snap: &TabRestartSnapshot) -> bool {
        let tab = self.mux.read().get_tab(&snap.tab_id).cloned();
        tab.is_some_and(|tab| {
            self.restart_session_exists(&tab, &snap.provider, snap.session_id.as_deref())
                == Some(true)
        })
    }

    fn finish_observed_restart(
        &self,
        mut outcome: RestartOutcome,
        replaced_pty_id: Option<&str>,
        timeout: Duration,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        match crate::service::boot_stage::wait_for_restart_readiness(
            &self.mux,
            &outcome.tab_id,
            replaced_pty_id,
            timeout,
        ) {
            crate::service::boot_stage::RestartReadiness::Ready { pty_id } => {
                outcome.agent_ready = true;
                outcome.new_pty_id = Some(pty_id);
                self.log_restart_timeline("ready", &outcome);
                Ok(outcome)
            }
            crate::service::boot_stage::RestartReadiness::Failed { pty_id, reason } => {
                outcome.new_pty_id = pty_id;
                self.log_restart_timeline("failed", &outcome);
                Err(RestartAttemptError::Failed {
                    outcome,
                    reason,
                    timed_out: false,
                })
            }
            crate::service::boot_stage::RestartReadiness::TimedOut { pty_id } => {
                outcome.new_pty_id = pty_id;
                self.log_restart_timeline("timed_out", &outcome);
                Err(RestartAttemptError::Failed {
                    outcome,
                    reason: format!(
                        "restart accepted but new agent readiness was not observed within {} seconds",
                        timeout.as_secs()
                    ),
                    timed_out: true,
                })
            }
        }
    }

    fn log_restart_timeline(&self, result: &str, outcome: &RestartOutcome) {
        let (current_pty, boot_stage, agent_booting, session_id, current_nonce, reported_nonce) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(&outcome.tab_id);
            (
                tab.and_then(|tab| tab.pty_id.clone()),
                tab.and_then(|tab| tab.boot_stage),
                tab.is_some_and(|tab| tab.agent_booting),
                tab.and_then(|tab| tab.agent_session_id.clone()),
                tab.and_then(|tab| tab.session_binding_nonce.clone()),
                tab.and_then(|tab| tab.reported_session_binding_nonce.clone()),
            )
        };
        log::info!(
            "[delivery-timeline] event=restart-return result={} tab={} old_pty={} \
             new_pty={} current_pty={} boot_stage={:?} agent_booting={} session={} \
             current_nonce_present={} reported_nonce_present={} binding_matches={}",
            result,
            outcome.tab_id,
            outcome.old_pty_id.as_deref().unwrap_or("-"),
            outcome.new_pty_id.as_deref().unwrap_or("-"),
            current_pty.as_deref().unwrap_or("-"),
            boot_stage,
            agent_booting,
            session_id.as_deref().unwrap_or("-"),
            current_nonce.is_some(),
            reported_nonce.is_some(),
            current_nonce.is_some() && current_nonce == reported_nonce,
        );
    }

    fn old_pty_was_torn_down(&self, snap: &TabRestartSnapshot) -> bool {
        if snap.pty_id.is_empty() {
            return false;
        }
        let tab_released = self
            .mux
            .read()
            .get_tab(&snap.tab_id)
            .and_then(|tab| tab.pty_id.as_deref())
            != Some(snap.pty_id.as_str());
        tab_released && self.pty.lock().get_session(&snap.pty_id).is_none()
    }
}

fn require_observed_live_agent(liveness: AgentLiveness) -> Result<(), AgentRestartError> {
    match liveness {
        AgentLiveness::Live => Ok(()),
        AgentLiveness::Absent => Err(AgentRestartError::BadRequest(
            "agent process is absent; restart is available only for an observed live agent"
                .to_string(),
        )),
        AgentLiveness::Unobserved => Err(AgentRestartError::BadRequest(
            "agent process has not been observed; restart is not yet available".to_string(),
        )),
    }
}

/// Resume provenance gate: fires ONLY when hook-reported transcript evidence
/// exists for the snapshot's session. Evidenced-but-missing ⇒ declared failure
/// before any command is emitted; no evidence ⇒ None, today's behavior.
fn evidenced_transcript_block(snap: &TabRestartSnapshot) -> Option<String> {
    let session_id = snap.session_id.as_deref()?;
    let stored = codeterm_plugin::session_evidence::stored_transcript_path(session_id)?;
    if codeterm_plugin::session_evidence::transcript_exists(&stored) {
        return None;
    }
    Some(format!(
        "resume unavailable: transcript missing for session {} (evidenced path {}); \
         relaunch fresh to start a new session in this tab",
        session_id,
        stored.display()
    ))
}

mod account;
pub mod exit_recovery;
pub mod failed_launch;
pub mod fresh;
pub mod options;
pub mod provider_switch;
pub use provider_switch::{
    AcceptedProviderSwitch, ProviderSwitchOperations, ProviderSwitchOutcome, ProviderSwitchRequest,
    ProviderSwitchStatus,
};
pub mod launch_command;
pub mod model_switch;
pub mod relaunch;
pub mod runtime;
use launch_command::build_relaunch_command;
#[cfg(test)]
use launch_command::build_relaunch_command_with_registry;
pub mod shutdown;
pub mod unverified_shell;

#[cfg(test)]
mod tests;

pub mod switch_confirmation;
