use super::{dispatch_failed, emit_plan_changed, roll_back_dispatch_attempt, JsonRpcError};
use codeterm_service_core::multiplexer::types::AgentRole;
#[cfg(test)]
use codeterm_service_core::plan::TaskExecutionAttempt;
use codeterm_service_core::service::plan::PlanStore;
use codeterm_service_core::service::{ProvisionedWorkerLane, ServiceState};

pub(crate) struct DispatchSeed<'a> {
    pub svc: &'a ServiceState,
    pub store: &'a PlanStore,
    pub plan_id: &'a str,
    pub task_id: &'a str,
    pub parent_tab_id: &'a str,
    pub tab_id: &'a str,
    pub attempt_id: &'a str,
    pub seed: &'a codeterm_service_core::service::plan::TaskDispatchSeedRead,
    pub rollback: &'a codeterm_service_core::service::plan::DispatchUndo,
    pub provisioned_lane: Option<ProvisionedWorkerLane>,
    pub now: u64,
}

pub(crate) fn seed_acceptance_or_rollback(
    seed: DispatchSeed<'_>,
) -> Result<Option<ProvisionedWorkerLane>, JsonRpcError> {
    let DispatchSeed {
        svc,
        store,
        plan_id,
        task_id,
        parent_tab_id,
        tab_id,
        attempt_id,
        seed,
        rollback,
        provisioned_lane,
        now,
    } = seed;
    let predecessors = seed
        .predecessor_tabs
        .iter()
        .filter(|tab| tab.as_str() != tab_id)
        .cloned()
        .collect::<Vec<_>>();
    if seed.acceptance_criteria.is_empty() && predecessors.is_empty() {
        return Ok(provisioned_lane);
    }
    let placement = {
        let mut mux = svc.mux.write();
        mux.set_tab_role_kind(tab_id, AgentRole::Worker, None, Some(parent_tab_id))
    };
    let seeded = placement.and_then(|()| {
        codeterm_service_core::async_runtime::block_on(svc.tab_todo_inherit_or_seed(
            tab_id,
            parent_tab_id,
            &predecessors,
            &seed.acceptance_criteria,
        ))
        .map(|_| ())
    });
    if let Err(error) = seeded {
        let cleanup = roll_back_dispatch_attempt(
            store,
            rollback,
            tab_id,
            attempt_id,
            provisioned_lane,
            || super::close_tab_if_present(svc, tab_id),
            now,
        );
        log::warn!(
            "[task-dispatch] dispatch_startup_failed plan={plan_id} task={task_id} tab={tab_id} attempt={attempt_id} stage=todo_seed cause={error}"
        );
        emit_plan_changed(svc, plan_id);
        return Err(dispatch_failed(
            "todo_seed",
            match cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; {cleanup}"),
            },
        ));
    }
    Ok(provisioned_lane)
}

/// Finished local executors of the same task, newest first; their checklist
/// carries over. Live and competition attempts keep their own lists.
#[cfg(test)]
fn predecessor_tab_ids(attempts: &[TaskExecutionAttempt], new_tab_id: &str) -> Vec<String> {
    let mut tab_ids: Vec<String> = Vec::new();
    for attempt in attempts.iter().rev().filter(|attempt| {
        (attempt.closed_at.is_some() || attempt.retired_at.is_some())
            && attempt.competition_group_id.is_none()
            && attempt.mesh_peer.is_none()
    }) {
        let tab_id = attempt.executor_tab_id.trim();
        if tab_id.is_empty() || tab_id == new_tab_id || tab_ids.iter().any(|id| id == tab_id) {
            continue;
        }
        tab_ids.push(tab_id.to_owned());
    }
    tab_ids
}

#[cfg(test)]
mod tests;
