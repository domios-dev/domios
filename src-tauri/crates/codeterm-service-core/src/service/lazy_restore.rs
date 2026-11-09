use crate::multiplexer::types::{Tab, TabBacking};
use crate::service::MuxEmitCause;
use crate::service::ServiceContext;
use codeterm_pty::multiplexer::restore_policy::{restore_on_boot, RestoreDecision};
use codeterm_types::pty::BootStage;
use std::collections::HashSet;
use std::sync::LazyLock;

static WAKING: LazyLock<parking_lot::Mutex<HashSet<String>>> = LazyLock::new(Default::default);

mod policy;
#[cfg(test)]
mod read_failure_tests;
mod user_send;

#[cfg(test)]
pub(crate) mod test_boundary;

struct WakeRequest(String);

#[derive(Clone, Copy, PartialEq, Eq)]
enum WakeIntent {
    Resume,
    Restart,
}

impl Drop for WakeRequest {
    fn drop(&mut self) {
        WAKING.lock().remove(&self.0);
    }
}

/// Reloading a session the provider refused cannot succeed; only the owner's
/// Start new session or an explicit restart moves such a tab on.
fn provider_lost_session(tab: &Tab) -> bool {
    tab.agent_runtime_state.as_ref().is_some_and(|state| {
        state["state"] == "degraded" && state["failureCode"] == "session_load_refused"
    })
}

pub(crate) fn wake_eligible(tab: &Tab, explicit: bool) -> bool {
    tab.frozen
        || explicit
            && !tab.protected
            && !provider_lost_session(tab)
            && (tab.boot_stage == Some(BootStage::SpawnFailed)
                || tab.restored_from_snapshot
                    && matches!(tab.backing, TabBacking::ChatBackend { .. })
                || matches!(tab.backing, TabBacking::Pty)
                    && tab.pty_id.is_none()
                    && !tab
                        .close_restore
                        .as_ref()
                        .is_some_and(|closed| closed.stopped))
}

pub fn initialize(svc: &ServiceContext, active_window_hours: u64) {
    initialize_with_continuing(svc, active_window_hours, |tab| {
        svc.plan_store
            .continuing_restore_tab_projection(tab)
            .map_err(|error| error.to_string())
    });
}

fn initialize_with_continuing(
    svc: &ServiceContext,
    active_window_hours: u64,
    read: impl Fn(&str) -> Result<bool, String>,
) {
    let snapshot = svc.mux.read().snapshot();
    let continuing = snapshot
        .tabs
        .values()
        .map(|tab| {
            let read =
                if tab.agent_type.is_none() && tab.role.is_none() && tab.task_binding.is_none() {
                    Ok(false)
                } else {
                    read(&tab.id)
                };
            (tab.id.clone(), read)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64;
    let window_ms = active_window_hours.saturating_mul(3_600_000);
    let mut mux = svc.mux.write();
    let general_workspace = super::general_agent::general_workspace_id(&mux);
    let snapshot = mux.snapshot();
    for workspace in &snapshot.workspaces {
        for id in &workspace.tabs {
            if let Some(tab) = mux.get_tab_mut(id) {
                let is_continuing = match continuing.get(id) {
                    Some(Ok(continuing)) => *continuing,
                    Some(Err(_)) => {
                        tab.frozen = tab.restored_from_snapshot && !tab.protected;
                        continue;
                    }
                    None => false,
                };
                tab.frozen = tab.restored_from_snapshot
                    && !tab.protected
                    && policy::decision(
                        tab,
                        is_continuing,
                        restore_on_boot(
                            general_workspace.as_deref() == Some(workspace.id.as_str()),
                            tab.protected,
                            tab.last_active_at_ms,
                            now_ms,
                            window_ms,
                        ),
                    ) == RestoreDecision::Freeze;
            }
        }
    }
    drop(mux);
    for (tab, error) in continuing {
        if let Err(error) = error {
            svc.warn_plan_environment_read(&tab,format!("Automatic restore for this tab was deferred because plan state could not be read: {error}"));
        }
    }
}

impl ServiceContext {
    pub(crate) fn thaw_tab_for_restart(&self, tab_id: &str) -> Result<(), String> {
        wake_with_intent(self, tab_id, false, WakeIntent::Restart, |_, _| Ok(()))
    }

    pub fn wake_frozen_tab(&self, tab_id: &str) -> Result<(), String> {
        self.wake_tab(tab_id, false)
    }

    pub fn wake_tab_on_request(&self, tab_id: &str) -> Result<(), String> {
        self.wake_tab(tab_id, true)
    }

    fn wake_tab(&self, tab_id: &str, allow_failed: bool) -> Result<(), String> {
        wake_with(self, tab_id, allow_failed, |id, retry_started_agent| {
            #[cfg(test)]
            if let Some(result) = test_boundary::materialize(id) {
                return result;
            }
            if retry_started_agent {
                return self
                    .agent_restart(id, false, super::agent_restart::DEFAULT_GRACE_SECS)
                    .map(|_| ())
                    .map_err(|reason| match reason {
                        super::agent_restart::AgentRestartError::NotFound(reason)
                        | super::agent_restart::AgentRestartError::BadRequest(reason)
                        | super::agent_restart::AgentRestartError::Internal(reason) => reason,
                        other => format!("{other:?}"),
                    });
            }
            if self.tab_chat_backing(id).is_some() {
                self.chatbackend_reopen_if_restored(id).map(|_| ())
            } else {
                let cwd = self.mux.read().get_tab(id).and_then(|tab| tab.cwd.clone());
                self.validate_request_cwd(None, Some(id), cwd.as_deref())
                    .map_err(|error| error.data().to_string())?;
                self.ensure_tab_pty_for_send(id)
            }
        })
    }
}

fn wake_with(
    svc: &ServiceContext,
    tab_id: &str,
    allow_failed: bool,
    materialize: impl FnOnce(&str, bool) -> Result<(), String>,
) -> Result<(), String> {
    wake_with_intent(svc, tab_id, allow_failed, WakeIntent::Resume, materialize)
}

fn wake_with_intent(
    svc: &ServiceContext,
    tab_id: &str,
    allow_failed: bool,
    intent: WakeIntent,
    materialize: impl FnOnce(&str, bool) -> Result<(), String>,
) -> Result<(), String> {
    let retirement = {
        let mut mux = svc.mux.write();
        let resolved = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab {tab_id} not found"))?;
        if let Some(tab) = mux.get_tab_mut(&resolved) {
            tab.record_activity();
        }
        super::agent_restart::sleep_transition::request_wake(
            svc.ext::<super::agent_restart::RestartTracker>(),
            &resolved,
        )
    };
    super::agent_restart::sleep_transition::wait_for_retirement(
        retirement,
        super::agent_restart::sleep_transition::wake_deadline(),
    )?;
    let id = {
        let mux = svc.mux.read();
        let id = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab {tab_id} not found"))?;
        if !mux
            .get_tab(&id)
            .is_some_and(|tab| wake_eligible(tab, allow_failed))
        {
            return Ok(());
        }
        id
    };
    if !WAKING.lock().insert(id.clone()) {
        return Ok(());
    }
    let _request = WakeRequest(id.clone());
    let bypass = svc
        .mux
        .read()
        .get_tab(&id)
        .is_some_and(|tab| tab.restorable_launch.permission_bypass_requested);
    svc.require_neighbor_restore_grant(&id, bypass)?;
    let (retry_started_agent, retained_pty_progress) = {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(&id).ok_or("Sleeping tab closed")?;
        if !wake_eligible(tab, allow_failed) {
            return Ok(());
        }
        let retry_started_agent = !tab.frozen
            && tab.boot_stage == Some(BootStage::SpawnFailed)
            && !tab.restored_from_snapshot
            && tab.agent_type.is_some();
        tab.frozen = false;
        tab.record_activity();
        let chat_backend = matches!(
            tab.backing,
            crate::multiplexer::types::TabBacking::ChatBackend { .. }
        );
        let retained_pty_progress = intent == WakeIntent::Resume
            && !retry_started_agent
            && !chat_backend
            && tab.pty_id.is_some();
        if !retained_pty_progress {
            let stage = if chat_backend {
                BootStage::AgentLaunching
            } else {
                BootStage::PtyStarting
            };
            super::boot_stage::apply_boot_stage(tab, stage, None);
        }
        (retry_started_agent, retained_pty_progress)
    };
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
    let result = materialize(&id, retry_started_agent);
    if let Err(reason) = &result {
        if let Some(tab) = svc.mux.write().get_tab_mut(&id) {
            tab.agent_booting = false;
            super::boot_stage::apply_boot_stage(tab, BootStage::SpawnFailed, Some(reason));
        }
    }
    if result.is_err() || retained_pty_progress {
        svc.wake_agent_deliveries_on_state_change(&id);
    }
    if result.is_ok() && retained_pty_progress {
        svc.wake_user_owner_on_state_change(&id);
    }
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
    result
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod wake_readiness_tests;
