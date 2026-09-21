use super::{plan_rpc_err, JsonRpcError};
use crate::multiplexer::types::TabCloseReason;
use crate::service::plan::{PlanError, PlanStore};
use crate::service::ServiceState;

pub(super) struct DispatchReservationTab<'a> {
    svc: &'a ServiceState,
    tab_id: String,
    cleanup_on_drop: bool,
}

impl<'a> DispatchReservationTab<'a> {
    pub(super) fn new(svc: &'a ServiceState, tab_id: String) -> Self {
        DispatchReservationTab {
            svc,
            tab_id,
            cleanup_on_drop: true,
        }
    }

    pub(super) fn mark_spawned(&mut self) {
        self.cleanup_on_drop = false;
    }

    pub(super) fn close(&mut self) -> Result<(), String> {
        let result = close_tab_if_present(self.svc, &self.tab_id);
        if result.is_ok() {
            self.cleanup_on_drop = false;
        }
        result
    }
}

impl Drop for DispatchReservationTab<'_> {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            if let Err(error) = close_tab_if_present(self.svc, &self.tab_id) {
                log::warn!(
                    "[task-dispatch] reserved Tab cleanup failed tab={}: {error}",
                    self.tab_id
                );
            }
        }
    }
}

pub(super) fn close_tab_if_present(svc: &ServiceState, tab_id: &str) -> Result<(), String> {
    if svc.mux.read().get_tab(tab_id).is_none() {
        Ok(())
    } else {
        svc.close_tab_with_reason(tab_id, TabCloseReason::SpawnRollback)
    }
}

pub(crate) fn dispatch_startup_owner_is_live(
    svc: &ServiceState,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
) -> bool {
    let (pty_id, agent_declared, agent_booting, task_binding_matches) = {
        let mux = svc.mux.read();
        let Some(tab) = mux.get_tab(tab_id) else {
            return false;
        };
        (
            tab.pty_id.clone(),
            tab.agent_type.is_some(),
            tab.agent_booting,
            tab.task_binding.as_ref().is_some_and(|binding| {
                binding.plan_id.as_deref() == Some(plan_id)
                    && binding.task_id.as_deref() == Some(task_id)
            }),
        )
    };
    if !agent_declared || !task_binding_matches {
        return false;
    }
    if crate::agent::live_state::observe_tab_liveness(svc, tab_id)
        == codeterm_types::pty::AgentLiveness::Live
    {
        return true;
    }
    agent_booting && pty_id.is_some_and(|pty_id| svc.pty.lock().get_session(&pty_id).is_some())
}

pub(super) fn refuse_live_dispatch_startup_owner(
    store: &PlanStore,
    svc: &ServiceState,
    plan_id: &str,
    task_id: &str,
) -> Result<(), JsonRpcError> {
    let Some(owner) = store.dispatch_startup_owner_candidate(plan_id, task_id) else {
        return Ok(());
    };
    if !dispatch_startup_owner_is_live(svc, plan_id, task_id, &owner.tab_id) {
        return Ok(());
    }
    Err(plan_rpc_err(PlanError::DispatchInFlight {
        attempt_id: owner.attempt_id,
        tab_id: owner.tab_id,
    }))
}
