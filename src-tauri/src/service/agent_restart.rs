//! Agent restart: graceful /exit + auto-resume, force-kill PTY + relaunch, or
//! in-place recovery of a provider that exited to its shell.
//!
//! Wraps the manual /exit + resume dance documented in AGENT.md as a single
//! atomic primitive callable from the CLI (`codeterm agent restart`) and from
//! the self-update flow (`feat/auto-update`) which restarts every agent tab
//! around binary swap.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_types::pty::AgentLiveness;
use parking_lot::Mutex;

use super::ServiceContext;
use crate::agent::provider::{AgentLaunchParams, StructuredPromptServer};

/// Minimum seconds between consecutive restarts for a single tab.
pub const RESTART_RATE_LIMIT_SECS: u64 = 30;

/// Default grace period before tearing down the old PTY.
pub const DEFAULT_GRACE_SECS: u64 = 10;

/// Failure deadline for the new PTY to reach AgentReady.
pub const RESTART_READY_TIMEOUT_SECS: u64 = 45;

/// Tracks restart attempts and serializes in-flight recovery per tab.
/// Lives on `ServiceContext` and survives across requests.
#[derive(Default)]
pub struct RestartTrackerState {
    last_restart: HashMap<String, Instant>,
    in_progress: HashSet<String>,
}

pub type RestartTracker = Arc<Mutex<RestartTrackerState>>;

pub(super) struct RestartClaim {
    tracker: RestartTracker,
    tab_id: String,
}

impl Drop for RestartClaim {
    fn drop(&mut self) {
        self.tracker.lock().in_progress.remove(&self.tab_id);
    }
}

pub fn new_restart_tracker() -> RestartTracker {
    Arc::new(Mutex::new(RestartTrackerState::default()))
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

#[derive(Debug, Clone)]
pub enum AgentRestartError {
    NotFound(String),
    BadRequest(String),
    RateLimited { retry_after_secs: u64 },
    Internal(String),
}

#[derive(Debug, Clone)]
pub struct RestartOutcome {
    pub tab_id: String,
    pub provider: String,
    pub session_id: Option<String>,
    pub session_was_stale: bool,
    pub force: bool,
    pub auto_approve: bool,
    pub restart_accepted: bool,
    pub old_pty_torn_down: bool,
    pub agent_ready: bool,
    /// The tab's plan/task caller binding was re-established against its own
    /// authoritative session. Always settled before `agent_ready` is returned.
    pub binding_restored: bool,
    /// An exited provider was relaunched in its own shell PTY; nothing was torn down.
    pub recovered_in_place: bool,
    pub old_pty_id: Option<String>,
    pub new_pty_id: Option<String>,
}

impl RestartOutcome {
    fn accepted(snap: &TabRestartSnapshot, session_was_stale: bool, force: bool) -> Self {
        Self {
            tab_id: snap.tab_id.clone(),
            provider: snap.provider.clone(),
            session_id: snap.session_id.clone(),
            session_was_stale,
            force,
            auto_approve: snap.auto_approve,
            restart_accepted: true,
            old_pty_torn_down: false,
            agent_ready: false,
            binding_restored: false,
            recovered_in_place: false,
            old_pty_id: (!snap.pty_id.is_empty()).then(|| snap.pty_id.clone()),
            new_pty_id: None,
        }
    }

    /// Explicit session contract: resume the recorded session, or start fresh.
    pub fn session_action(&self) -> &'static str {
        match (&self.session_id, self.session_was_stale) {
            (Some(_), false) => "resume",
            _ => "fresh",
        }
    }
}

/// Tab state captured before any restart leg mutates it.
#[derive(Debug, Clone)]
struct TabRestartSnapshot {
    tab_id: String,
    pty_id: String,
    provider: String,
    session_id: Option<String>,
    cwd: Option<String>,
    auto_approve: bool,
    role: Option<String>,
    role_profile: Option<String>,
}

pub(crate) struct ProviderUpdatePause {
    snap: TabRestartSnapshot,
    launch_cmd: String,
    _claim: RestartClaim,
}

#[derive(Debug)]
pub(crate) enum RestartAttemptError {
    Request(AgentRestartError),
    Failed {
        outcome: RestartOutcome,
        reason: String,
        timed_out: bool,
    },
}

impl From<AgentRestartError> for RestartAttemptError {
    fn from(error: AgentRestartError) -> Self {
        Self::Request(error)
    }
}

impl ServiceContext {
    pub(crate) fn pause_for_provider_update(
        &self,
        tab_id: &str,
        expected_provider: &str,
        expected_session: &str,
    ) -> Result<ProviderUpdatePause, String> {
        let snap = self
            .snapshot_tab_for_restart(tab_id)
            .map_err(|error| format!("{error:?}"))?;
        if snap.provider != expected_provider
            || snap.session_id.as_deref() != Some(expected_session)
        {
            return Err(format!(
                "Tab {} changed provider or session before update",
                snap.tab_id
            ));
        }
        if !self.session_file_present(&snap) {
            return Err(format!(
                "Tab {} no longer has its bound session",
                snap.tab_id
            ));
        }
        if !self.provider_update_tab_is_idle(&snap.tab_id) {
            return Err(format!("Tab {} is no longer idle", snap.tab_id));
        }
        let claim = self
            .claim_restart(&snap.tab_id, true)
            .map_err(|error| format!("{error:?}"))?;
        let launch_cmd = build_relaunch_command(self, &snap, true, None)
            .map(|command| crate::agent::stub_launch::apply(&snap.provider, command))
            .ok_or_else(|| format!("provider {} returned no resume command", snap.provider))?;
        if !self.provider_update_tab_is_idle(&snap.tab_id) {
            return Err(format!(
                "Tab {} became busy before it could be paused",
                snap.tab_id
            ));
        }
        self.request_graceful_exit(&snap, false, Duration::from_secs(DEFAULT_GRACE_SECS));
        self.tear_down_restart_pty(&snap)?;
        Ok(ProviderUpdatePause {
            snap,
            launch_cmd,
            _claim: claim,
        })
    }

    fn provider_update_tab_is_idle(&self, tab_id: &str) -> bool {
        let Some(snapshot) = self.try_snapshot_with_reconciled_status() else {
            return false;
        };
        snapshot.tab_statuses.get(tab_id) == Some(&codeterm_types::pty::TabStatus::Idle)
    }

    pub(crate) fn resume_after_provider_update(
        &self,
        pause: ProviderUpdatePause,
    ) -> Result<(), String> {
        if !self.session_file_present(&pause.snap) {
            return Err(format!(
                "Tab {} no longer has session {}",
                pause.snap.tab_id,
                pause.snap.session_id.as_deref().unwrap_or("<unknown>")
            ));
        }
        let outcome = RestartOutcome::accepted(&pause.snap, false, false);
        self.set_and_lock_tab_agent_type_from(
            &pause.snap.tab_id,
            pause.snap.provider.clone(),
            crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
            None,
        );
        crate::service::orchestrator::launch_agent_in_background(
            self.clone(),
            pause.snap.tab_id.clone(),
            pause.launch_cmd,
            pause.snap.provider.clone(),
            crate::service::launch_diagnostics::AgentLaunchOrigin::Resume,
            None,
            None,
            None,
            None,
        );
        let ready = self
            .finish_observed_restart(
                outcome,
                Some(pause.snap.pty_id.as_str()),
                Duration::from_secs(RESTART_READY_TIMEOUT_SECS),
            )
            .map_err(|error| format!("{error:?}"))?;
        let result = self
            .settle_restored_binding(ready, &pause.snap)
            .map_err(|error| format!("{error:?}"))?;
        if result.session_id.as_deref() != pause.snap.session_id.as_deref() {
            return Err(format!(
                "Tab {} resumed a different session",
                pause.snap.tab_id
            ));
        }
        drop(pause._claim);
        Ok(())
    }

    pub fn agent_restart(
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

    pub(crate) fn agent_restart_observed(
        &self,
        tab_id: &str,
        force: bool,
        grace_secs: u64,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        if let Some(outcome) = self.restart_general_agent_if_target(tab_id, force)? {
            return outcome;
        }
        if let Some(exited_tab_id) = self.absent_restart_target(tab_id)? {
            return self.recover_exited_provider(&exited_tab_id, force, false);
        }
        let snap = self.snapshot_tab_for_restart(tab_id)?;
        if let Some(reason) = evidenced_transcript_block(&snap) {
            return Err(AgentRestartError::BadRequest(reason).into());
        }

        let restart_claim = self.claim_restart(&snap.tab_id, false)?;
        self.record_lifecycle(
            &snap.tab_id,
            crate::session::lifecycle::LifecycleTransition::RestartRequested,
        );

        let session_was_stale = !self.session_file_present(&snap);
        let mut outcome = RestartOutcome::accepted(&snap, session_was_stale, force);
        let session_present = !session_was_stale && snap.session_id.is_some();
        let launch_task = (!session_present)
            .then(|| {
                crate::service::orchestrator::provider_launch_task(
                    &self.agent_registry,
                    &snap.provider,
                    None,
                    snap.role.as_deref(),
                    snap.role_profile.as_deref(),
                )
            })
            .flatten();
        let Some(launch_cmd) =
            build_relaunch_command(self, &snap, session_present, launch_task.as_deref())
                .map(|command| crate::agent::stub_launch::apply(&snap.provider, command))
        else {
            return Err(RestartAttemptError::Failed {
                outcome,
                reason: format!("provider {} returned no launch command", snap.provider),
                timed_out: false,
            });
        };

        self.request_graceful_exit(&snap, force, Duration::from_secs(grace_secs.max(1)));
        if let Err(reason) = self.tear_down_restart_pty(&snap) {
            return Err(RestartAttemptError::Failed {
                outcome,
                reason,
                timed_out: false,
            });
        }
        outcome.old_pty_torn_down = true;

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
            if session_present {
                crate::service::launch_diagnostics::AgentLaunchOrigin::Resume
            } else {
                crate::service::launch_diagnostics::AgentLaunchOrigin::Restart
            },
            launch_task,
            None,
            None,
            None,
        );
        drop(restart_claim);

        let ready = self.finish_observed_restart(
            outcome,
            Some(snap.pty_id.as_str()),
            Duration::from_secs(RESTART_READY_TIMEOUT_SECS),
        )?;
        self.settle_restored_binding(ready, &snap)
    }

    /// Rebind callers before readiness is reported: a tab that cannot regain
    /// its exact binding must not be handed back as continuable.
    fn settle_restored_binding(
        &self,
        mut outcome: RestartOutcome,
        snap: &TabRestartSnapshot,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        // Only a resumed session has an id the tab must still be holding; a
        // fresh launch legitimately comes back on a new one.
        let expected = (outcome.session_action() == "resume")
            .then_some(snap.session_id.as_deref())
            .flatten();
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
            Err(error) => Err(RestartAttemptError::Failed {
                outcome,
                reason: error.to_string(),
                timed_out: false,
            }),
        }
    }

    fn restart_general_agent_if_target(
        &self,
        tab_id: &str,
        force: bool,
    ) -> Result<Option<Result<RestartOutcome, RestartAttemptError>>, AgentRestartError> {
        let Some(target) = self.general_agent_restart_snapshot(tab_id) else {
            return Ok(None);
        };
        self.require_tab_restart_liveness(&target.tab_id)?;
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

        let session_was_stale = !self.session_file_present(&target);
        let outcome = RestartOutcome::accepted(&target, session_was_stale, force);
        let mux = self.mux.clone();
        let svc = Arc::new(self.clone());
        let pty = self.pty.clone();
        let screen_state = self.screen_state.clone();
        let app = self.app_handle.clone();
        let config = {
            use tauri::Manager;
            self.app_handle
                .state::<crate::config::ConfigState>()
                .inner()
                .clone()
        };
        std::thread::spawn(move || {
            crate::service::general_agent::restart_general_agent(
                &svc,
                &mux,
                &pty,
                &screen_state,
                &config,
                &app,
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
            use tauri::Manager;
            Some(
                self.app_handle
                    .state::<crate::config::ConfigState>()
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
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            auto_approve: false,
            role: tab.role.map(|role| format!("{role:?}")),
            role_profile: tab.role_profile.clone(),
        })
    }

    fn snapshot_tab_for_restart(
        &self,
        tab_id: &str,
    ) -> Result<TabRestartSnapshot, AgentRestartError> {
        self.require_tab_restart_liveness(tab_id)?;

        let mut mux = self.mux.write();
        let resolved = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;
        let tab = mux
            .get_tab(&resolved)
            .cloned()
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;

        let provider = tab
            .agent_type
            .clone()
            .ok_or_else(|| AgentRestartError::BadRequest("no agent in this Tab".to_string()))?;
        let pty_id = tab
            .pty_id
            .clone()
            .ok_or_else(|| AgentRestartError::BadRequest("Tab has no PTY".to_string()))?;

        if self
            .agent_registry
            .with_provider(&provider, |_| ())
            .is_none()
        {
            return Err(AgentRestartError::BadRequest(format!(
                "unknown agent provider: {}",
                provider
            )));
        }

        let auto_approve = crate::service::permission_policy::configured_permission_bypass(
            &self.agent_registry,
            &provider,
        )
        .map_err(AgentRestartError::BadRequest)?
        .is_some();
        if let Some(live_tab) = mux.get_tab_mut(&resolved) {
            live_tab.auto_approve = auto_approve;
        }

        Ok(TabRestartSnapshot {
            tab_id: resolved,
            pty_id,
            provider,
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            auto_approve,
            role: tab.role.map(|role| format!("{role:?}")),
            role_profile: tab.role_profile.clone(),
        })
    }

    fn require_tab_restart_liveness(&self, tab_id: &str) -> Result<(), AgentRestartError> {
        let resolved = self
            .mux
            .read()
            .resolve_tab_id(tab_id)
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;
        let liveness = crate::agent::live_state::observe_tab_liveness(self, &resolved);
        if liveness == AgentLiveness::Absent {
            self.reconcile_tab_restored_without_agent(&resolved);
        }
        require_observed_live_agent(liveness)
    }

    pub(super) fn claim_restart(
        &self,
        tab_id: &str,
        automatic: bool,
    ) -> Result<RestartClaim, AgentRestartError> {
        claim_restart_attempt(&self.restart_tracker, tab_id, automatic)
    }

    fn session_file_present(&self, snap: &TabRestartSnapshot) -> bool {
        let (Some(cwd), Some(sid)) = (snap.cwd.as_deref(), snap.session_id.as_deref()) else {
            return false;
        };
        self.agent_registry
            .with_provider(&snap.provider, |p| p.session_exists(cwd, sid))
            .unwrap_or(false)
    }

    fn request_graceful_exit(&self, snap: &TabRestartSnapshot, force: bool, grace: Duration) {
        if force {
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
        if let Err(error) = self.send_text(&snap.tab_id, &exit_command) {
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

    fn tear_down_restart_pty(&self, snap: &TabRestartSnapshot) -> Result<(), String> {
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
            reaper.wait_bounded(Duration::from_secs(5))?;
        }
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
             current_nonce={} reported_nonce={}",
            result,
            outcome.tab_id,
            outcome.old_pty_id.as_deref().unwrap_or("-"),
            outcome.new_pty_id.as_deref().unwrap_or("-"),
            current_pty.as_deref().unwrap_or("-"),
            boot_stage,
            agent_booting,
            session_id.as_deref().unwrap_or("-"),
            current_nonce.as_deref().unwrap_or("-"),
            reported_nonce.as_deref().unwrap_or("-"),
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

/// Build the shell command that launches the agent inside the replacement PTY.
/// If the recorded session id is still valid, prefer the provider's resume
/// command. Otherwise fall back to a fresh launch — the spec calls for graceful
/// degradation on stale ids, not failure.
fn build_relaunch_command(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    session_present: bool,
    launch_task: Option<&str>,
) -> Option<String> {
    let provider = &snap.provider;
    let auto_approve = match crate::service::permission_policy::configured_permission_bypass(
        &svc.agent_registry,
        provider,
    ) {
        Ok(authorization) => authorization.is_some(),
        Err(error) => {
            log::error!(
                "[agent-restart] refusing policy downgrade for provider={provider}: {error}"
            );
            return None;
        }
    };
    let session_id = snap.session_id.as_deref();
    let cwd = snap.cwd.as_deref().unwrap_or_default();
    let canonical_project = snap.cwd.as_deref().and_then(|cwd| {
        crate::service::launch_session_detection::canonical_project_for_tab(
            &svc.plan_store,
            &snap.tab_id,
            cwd,
        )
    });
    let structured_prompt_server = svc
        .agent_registry
        .with_provider(provider, |p| p.needs_structured_prompt_server())
        .unwrap_or(false)
        .then(|| {
            crate::service::structured_prompt::ensure_for_spawn(svc, provider, &snap.tab_id, cwd)
        })
        .flatten();
    build_relaunch_command_with_registry(
        &svc.agent_registry,
        provider,
        session_id,
        auto_approve,
        session_present,
        structured_prompt_server,
        launch_task,
        &snap.tab_id,
        snap.cwd.as_deref(),
        canonical_project.as_deref(),
    )
}

fn build_relaunch_command_with_registry(
    registry: &crate::agent::registry::AgentRegistry,
    provider: &str,
    session_id: Option<&str>,
    auto_approve: bool,
    session_present: bool,
    structured_prompt_server: Option<StructuredPromptServer>,
    launch_task: Option<&str>,
    tab_id: &str,
    cwd: Option<&str>,
    canonical_project: Option<&str>,
) -> Option<String> {
    let starter_prompt = crate::agent::provider::StarterPrompt::NoStarter;
    if session_present && structured_prompt_server.is_some() {
        if let Some(sid) = session_id {
            let mut args = Vec::new();
            crate::service::launch_session_detection::append_sandbox_git_metadata_args(
                registry,
                provider,
                auto_approve,
                cwd,
                canonical_project,
                &mut args,
            );
            crate::service::launch_session_detection::append_sandbox_agent_pad_arg(
                registry,
                provider,
                auto_approve,
                &crate::paths::data_dir(),
                tab_id,
                &mut args,
            );
            return registry.with_provider(provider, |p| {
                p.build_launch_command(&AgentLaunchParams {
                    agent_name: None,
                    task: None,
                    starter_prompt,
                    args: (!args.is_empty()).then_some(args),
                    skip_permissions: auto_approve,
                    session_id: Some(sid.to_string()),
                    structured_prompt_server: structured_prompt_server.clone(),
                    launch_marker: None,
                    instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
                })
            });
        }
    }
    if session_present {
        if let Some(sid) = session_id {
            let system_prompt = registry
                .with_provider(provider, |p| {
                    crate::service::launch_delivery::content_for_descriptor(
                        &p.system_prompt_delivery(),
                        launch_task,
                    )
                    .system_prompt
                })
                .flatten();
            let mut args = Vec::new();
            crate::service::launch_session_detection::append_sandbox_git_metadata_args(
                registry,
                provider,
                auto_approve,
                cwd,
                canonical_project,
                &mut args,
            );
            crate::service::launch_session_detection::append_sandbox_agent_pad_arg(
                registry,
                provider,
                auto_approve,
                &crate::paths::data_dir(),
                tab_id,
                &mut args,
            );
            let resume = crate::agent::provider::AgentResumeParams {
                session_id: sid.to_string(),
                skip_permissions: auto_approve,
                agent_name: None,
                system_prompt,
                instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
                args,
            };
            if let Some(cmd) = registry
                .with_provider(provider, |p| p.build_resume_command_with_context(&resume))
                .flatten()
            {
                return Some(cmd);
            }
        }
    }
    let mut params = AgentLaunchParams {
        agent_name: None,
        task: launch_task.map(str::to_string),
        starter_prompt,
        args: None,
        skip_permissions: auto_approve,
        session_id: None,
        structured_prompt_server,
        launch_marker: None,
        instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
    };
    if let Some(cwd) = cwd {
        let mut args = Vec::new();
        crate::service::launch_session_detection::append_sandbox_git_metadata_args(
            registry,
            provider,
            auto_approve,
            Some(cwd),
            canonical_project,
            &mut args,
        );
        crate::service::launch_session_detection::append_sandbox_agent_pad_arg(
            registry,
            provider,
            auto_approve,
            &crate::paths::data_dir(),
            tab_id,
            &mut args,
        );
        params.args = (!args.is_empty()).then_some(args);
    }
    crate::service::launch_session_detection::mark_fresh_launch(tab_id, &mut params);
    registry.with_provider(provider, |p| p.build_launch_command(&params))
}

pub(crate) fn screen_shows_agent_tui(
    registry: &crate::agent::registry::AgentRegistry,
    screen_text: &str,
) -> bool {
    crate::pty::screen_state::screen_has_agent_tui(screen_text)
        || registry.any_provider_screen_has_tui(screen_text)
}

mod exit_recovery;

#[cfg(test)]
mod tests;
