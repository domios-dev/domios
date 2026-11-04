use crate::plan::Task;

use super::{PlanError, PlanStore};

pub(super) fn validate_projected_gate_claim(
    authorized: Option<&[String]>,
    claimed_gate: &str,
) -> Result<(), PlanError> {
    let Some(authorized) = authorized else {
        return Ok(());
    };
    if authorized.iter().any(|gate| gate == claimed_gate) {
        return Ok(());
    }
    Err(PlanError::UnauthorizedGateClaim {
        claimed: claimed_gate.to_string(),
        authorized: authorized.to_vec(),
    })
}

pub(super) fn validate_authorized_gate_claim(
    task: &Task,
    reporter_tab: &str,
    claimed_gate: Option<&str>,
) -> Result<(), PlanError> {
    let Some(claimed_gate) = claimed_gate else {
        return Ok(());
    };
    if task.assigned_tab_id.as_deref() != Some(reporter_tab) {
        return Ok(());
    }
    let owner_epoch = task.interruption.owner_epoch;
    let Some(attempt) = task.execution_attempts.iter().rfind(|attempt| {
        attempt.executor_tab_id == reporter_tab
            && attempt.closed_at.is_none()
            && (owner_epoch == 0 || attempt.interruption.owner_epoch == owner_epoch)
    }) else {
        return Ok(());
    };
    validate_projected_gate_claim(attempt.authorized_gates.as_deref(), claimed_gate)
}

impl PlanStore {
    pub fn validate_worker_gate_claim(
        &self,
        plan_id: &str,
        task_id: &str,
        reporter_tab: &str,
        claimed_gate: Option<&str>,
    ) -> Result<(), PlanError> {
        let Some(claimed_gate) = claimed_gate else {
            return Ok(());
        };
        let authorized =
            self.database
                .worker_gate_claim_projection(plan_id, task_id, reporter_tab)?;
        validate_projected_gate_claim(authorized.as_deref(), claimed_gate)
    }
}
