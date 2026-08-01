//! Agent restart: graceful /exit + auto-resume, or force-kill PTY + relaunch.
//!
//! Wraps the manual /exit + resume dance documented in AGENT.md as a single
//! atomic primitive callable from the CLI (`codeterm agent restart`) and from
//! the self-update flow (`feat/auto-update`) which restarts every agent tab
//! around binary swap.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::ServiceContext;
use crate::agent::provider::{AgentLaunchParams, StructuredPromptServer};

/// Minimum seconds between consecutive restarts for a single tab.
pub const RESTART_RATE_LIMIT_SECS: u64 = 30;

/// Default grace period before tearing down the old PTY.
pub const DEFAULT_GRACE_SECS: u64 = 10;

/// Failure deadline for the new PTY to reach AgentReady.
pub const RESTART_READY_TIMEOUT_SECS: u64 = 45;

/// Tracks the last successful restart per tab for rate limiting.
/// Lives on `ServiceContext` and survives across requests.
pub type RestartTracker = Arc<Mutex<HashMap<String, Instant>>>;

pub fn new_restart_tracker() -> RestartTracker {
    Arc::new(Mutex::new(HashMap::new()))
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

    /// Permission/sandbox profile the relaunch preserves from the tab.
    pub fn permission_profile(&self) -> &'static str {
        if self.auto_approve {
            "yolo"
        } else {
            "sandboxed"
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
        let snap = self.snapshot_tab_for_restart(tab_id)?;

        self.check_and_record_restart(&snap.tab_id)?;

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

        self.set_and_lock_tab_agent_type(&snap.tab_id, snap.provider.clone());
        crate::service::orchestrator::launch_agent_in_background(
            self.clone(),
            snap.tab_id.clone(),
            launch_cmd,
            snap.provider.clone(),
            crate::service::launch_diagnostics::AgentLaunchOrigin::Restart,
            launch_task,
            None,
            None,
        );

        self.finish_observed_restart(
            outcome,
            &snap.pty_id,
            Duration::from_secs(RESTART_READY_TIMEOUT_SECS),
        )
    }

    fn restart_general_agent_if_target(
        &self,
        tab_id: &str,
        force: bool,
    ) -> Result<Option<Result<RestartOutcome, RestartAttemptError>>, AgentRestartError> {
        let Some(target) = self.general_agent_restart_snapshot(tab_id) else {
            return Ok(None);
        };
        self.check_and_record_restart(&target.tab_id)?;

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

        log::info!(
            "[agent_restart] Tab {} recovered via General Agent relaunch",
            target.tab_id
        );
        let result = self.finish_observed_restart(
            outcome,
            &target.pty_id,
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
            auto_approve: true,
            role: tab.role.map(|role| format!("{role:?}")),
            role_profile: tab.role_profile.clone(),
        })
    }

    fn snapshot_tab_for_restart(
        &self,
        tab_id: &str,
    ) -> Result<TabRestartSnapshot, AgentRestartError> {
        let mux = self.mux.write();
        let resolved = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;
        let tab = mux
            .get_tab(&resolved)
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

        Ok(TabRestartSnapshot {
            tab_id: resolved,
            pty_id,
            provider,
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            auto_approve: tab.auto_approve,
            role: tab.role.map(|role| format!("{role:?}")),
            role_profile: tab.role_profile.clone(),
        })
    }

    fn check_and_record_restart(&self, tab_id: &str) -> Result<(), AgentRestartError> {
        let mut tracker = self.restart_tracker.lock();
        let now = Instant::now();
        if let Some(last) = tracker.get(tab_id) {
            let elapsed = now.duration_since(*last);
            let limit = Duration::from_secs(RESTART_RATE_LIMIT_SECS);
            if elapsed < limit {
                let retry = (limit - elapsed).as_secs().max(1);
                return Err(AgentRestartError::RateLimited {
                    retry_after_secs: retry,
                });
            }
        }
        tracker.insert(tab_id.to_string(), now);
        Ok(())
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
        self.screen_state.lock().remove(&snap.pty_id);
        self.clear_tab_agent_identity_for_lifecycle_exit(&snap.tab_id);
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
        old_pty_id: &str,
        timeout: Duration,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        match crate::service::boot_stage::wait_for_restart_readiness(
            &self.mux,
            &outcome.tab_id,
            old_pty_id,
            timeout,
        ) {
            crate::service::boot_stage::RestartReadiness::Ready { pty_id } => {
                outcome.agent_ready = true;
                outcome.new_pty_id = Some(pty_id);
                Ok(outcome)
            }
            crate::service::boot_stage::RestartReadiness::Failed { pty_id, reason } => {
                outcome.new_pty_id = pty_id;
                Err(RestartAttemptError::Failed {
                    outcome,
                    reason,
                    timed_out: false,
                })
            }
            crate::service::boot_stage::RestartReadiness::TimedOut { pty_id } => {
                outcome.new_pty_id = pty_id;
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
    let auto_approve = snap.auto_approve;
    let session_id = snap.session_id.as_deref();
    let cwd = snap.cwd.as_deref().unwrap_or_default();
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
        snap.role.as_deref(),
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
    role: Option<&str>,
) -> Option<String> {
    let starter_prompt = crate::service::starter_prompt_for_role(role);
    if session_present && structured_prompt_server.is_some() {
        if let Some(sid) = session_id {
            return registry.with_provider(provider, |p| {
                p.build_launch_command(&AgentLaunchParams {
                    agent_name: None,
                    task: None,
                    starter_prompt,
                    args: None,
                    skip_permissions: auto_approve,
                    session_id: Some(sid.to_string()),
                    structured_prompt_server: structured_prompt_server.clone(),
                })
            });
        }
    }
    if session_present {
        if let Some(sid) = session_id {
            if let Some(cmd) = registry
                .with_provider(provider, |p| p.build_resume_command(sid, auto_approve))
                .flatten()
            {
                return Some(cmd);
            }
        }
    }
    registry.with_provider(provider, |p| {
        p.build_launch_command(&AgentLaunchParams {
            agent_name: None,
            task: launch_task.map(str::to_string),
            starter_prompt,
            args: None,
            skip_permissions: auto_approve,
            session_id: None,
            structured_prompt_server,
        })
    })
}

pub(crate) fn screen_shows_agent_tui(
    registry: &crate::agent::registry::AgentRegistry,
    screen_text: &str,
) -> bool {
    crate::pty::screen_state::screen_has_agent_tui(screen_text)
        || registry.any_provider_screen_has_tui(screen_text)
}

#[cfg(test)]
mod tests;
