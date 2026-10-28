use crate::multiplexer::types::{Tab, TabBacking};
use crate::service::MuxEmitCause;
use crate::service::ServiceContext;
use codeterm_pty::multiplexer::restore_policy::{restore_on_boot, RestoreDecision};
use codeterm_types::pty::BootStage;
use std::collections::HashSet;
use std::sync::LazyLock;

static WAKING: LazyLock<parking_lot::Mutex<HashSet<String>>> = LazyLock::new(Default::default);

mod policy;
mod user_send;

#[cfg(test)]
pub(crate) mod test_boundary;

struct WakeRequest(String);

impl Drop for WakeRequest {
    fn drop(&mut self) {
        WAKING.lock().remove(&self.0);
    }
}

fn wake_eligible(tab: &Tab, explicit: bool) -> bool {
    tab.frozen
        || explicit
            && !tab.protected
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
    let continuing = policy::continuing_tabs(&svc.plan_store.list_full());
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
                tab.frozen = tab.restored_from_snapshot
                    && !tab.protected
                    && policy::decision(
                        tab,
                        continuing.contains(id),
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
}

impl ServiceContext {
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
    let retry_started_agent = {
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
        let stage = if matches!(
            tab.backing,
            crate::multiplexer::types::TabBacking::ChatBackend { .. }
        ) {
            BootStage::AgentLaunching
        } else {
            BootStage::PtyStarting
        };
        super::boot_stage::apply_boot_stage(tab, stage, None);
        retry_started_agent
    };
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
    let result = materialize(&id, retry_started_agent);
    if let Err(reason) = &result {
        if let Some(tab) = svc.mux.write().get_tab_mut(&id) {
            tab.agent_booting = false;
            super::boot_stage::apply_boot_stage(tab, BootStage::SpawnFailed, Some(reason));
        }
    }
    if result.is_err() {
        svc.wake_agent_deliveries_on_state_change(&id);
    }
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
    result
}

#[cfg(test)]
mod tests;
