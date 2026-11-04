use super::{PlanError, PlanStore};
use crate::plan::{Plan, Task};
use std::cell::RefCell;

thread_local! {
    static COMMITTED_SNAPSHOT: RefCell<Option<Plan>> = const { RefCell::new(None) };
}

pub(in crate::service_plan) fn pending_after_verified(
    result: Result<(), PlanError>,
    verified: Result<(), PlanError>,
    _committed_id: Option<String>,
) -> Result<(), PlanError> {
    pending_after_readback(result, verified)
}

pub(in crate::service_plan) fn pending_after_readback<T>(
    result: Result<(), PlanError>,
    readback: Result<T, PlanError>,
) -> Result<T, PlanError> {
    match result {
        Ok(()) => readback,
        Err(PlanError::WritePending { .. }) => match readback {
            Ok(value) => Ok(value),
            Err(readback) => Err(readback),
        },
        Err(err) => Err(err),
    }
}

impl PlanStore {
    pub(in crate::service_plan) fn clear_committed_snapshot(&self) {
        COMMITTED_SNAPSHOT.with(|cell| *cell.borrow_mut() = None);
    }

    pub(in crate::service_plan) fn task_readback_error(
        operation: &str,
        message: impl Into<String>,
    ) -> PlanError {
        PlanError::ReadbackMismatch {
            operation: operation.into(),
            message: message.into(),
        }
    }

    // Re-reading after the plan lock drops races concurrent writers; verify the snapshot committed under the lock.
    pub(in crate::service_plan) fn record_committed_snapshot(&self, plan: &Plan) {
        COMMITTED_SNAPSHOT.with(|cell| *cell.borrow_mut() = Some(plan.clone()));
    }

    pub(in crate::service_plan) fn committed_plan_for(&self, plan_id: &str) -> Option<Plan> {
        match COMMITTED_SNAPSHOT.with(|cell| cell.borrow_mut().take()) {
            Some(plan) if plan.id == plan_id => Some(plan),
            _ => self.get(plan_id),
        }
    }

    pub(in crate::service_plan) fn read_task_for_verification(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<Task, PlanError> {
        let plan = self
            .committed_plan_for(plan_id)
            .ok_or(PlanError::NotFound)?;
        crate::plan::task_key::find_task(&plan.tasks, task_id)
            .cloned()
            .ok_or(PlanError::TaskNotFound)
    }

    pub(in crate::service_plan) fn verify_task_assignment(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: Option<&str>,
        taken_by_role: Option<&str>,
    ) -> Result<(), PlanError> {
        let task = self.read_task_for_verification(plan_id, task_id)?;
        if task.assigned_tab_id.as_deref() != tab_id {
            return Err(Self::task_readback_error(
                "task_assignment",
                format!("assigned_tab_id did not match for {}", task.id),
            ));
        }
        if task.taken_by_role.as_deref() != taken_by_role {
            return Err(Self::task_readback_error(
                "task_assignment",
                format!("taken_by_role did not match for {}", task.id),
            ));
        }
        if tab_id.is_none() && task.peer.is_some() {
            return Err(Self::task_readback_error(
                "task_assignment",
                format!("peer was not cleared for {}", task.id),
            ));
        }
        Ok(())
    }
}
