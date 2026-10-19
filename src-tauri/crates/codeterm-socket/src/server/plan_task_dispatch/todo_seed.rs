use super::{dispatch_failed, emit_plan_changed, roll_back_dispatch_attempt, JsonRpcError};
use codeterm_service_core::multiplexer::types::AgentRole;
use codeterm_service_core::plan::{PlanMode, Task};
use codeterm_service_core::service::plan::PlanStore;
use codeterm_service_core::service::{ProvisionedWorkerLane, ServiceState};

pub(super) struct DispatchSeed<'a> {
    pub svc: &'a ServiceState,
    pub store: &'a PlanStore,
    pub plan_id: &'a str,
    pub task_id: &'a str,
    pub parent_tab_id: &'a str,
    pub tab_id: &'a str,
    pub attempt_id: &'a str,
    pub original_task: &'a Task,
    pub original_plan_mode: &'a PlanMode,
    pub provisioned_lane: Option<ProvisionedWorkerLane>,
    pub now: u64,
}

pub(super) fn seed_acceptance_or_rollback(
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
        original_task,
        original_plan_mode,
        provisioned_lane,
        now,
    } = seed;
    if original_task.acceptance_criteria.is_empty() {
        return Ok(provisioned_lane);
    }
    let placement = {
        let mut mux = svc.mux.write();
        mux.set_tab_role_kind(tab_id, AgentRole::Worker, None, Some(parent_tab_id))
    };
    let seeded = placement.and_then(|()| {
        codeterm_service_core::async_runtime::block_on(svc.tab_todo_seed_from_acceptance(
            tab_id,
            parent_tab_id,
            &original_task.acceptance_criteria,
        ))
        .map(|_| ())
    });
    if let Err(error) = seeded {
        let cleanup = roll_back_dispatch_attempt(
            store,
            plan_id,
            original_task,
            original_plan_mode,
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
