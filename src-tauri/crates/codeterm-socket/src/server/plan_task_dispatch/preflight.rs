use super::lane_safety;
#[cfg(test)]
use super::lane_safety::preflight_checkout_occupancy;
use super::plan_rpc_err;
use codeterm_service_core::service::plan::milestone_gates::{
    regression_dispatch_decision, RegressionDispatchDecision,
};
use codeterm_service_core::service::{LaneProtection, ServiceState};
use serde_json::json;
#[cfg(test)]
use std::collections::HashMap;

use codeterm_service_core::service::plan::{PlanError, TaskDispatchPreflightRead};

use super::JsonRpcError;

pub(super) fn preflight_dispatch_task_status(
    _plan_id: &str,
    task: &TaskDispatchPreflightRead,
) -> Result<(), JsonRpcError> {
    match &task.status {
        codeterm_service_core::plan::TaskStatus::Pending
        | codeterm_service_core::plan::TaskStatus::InProgress
        | codeterm_service_core::plan::TaskStatus::Blocked => Ok(()),
        codeterm_service_core::plan::TaskStatus::Paused => Err(JsonRpcError {
            code: -32602,
            message: format!(
                "task '{}' is held by the system and cannot be dispatched",
                task.id
            ),
            data: Some(json!({
                "type": "invalid_task_dispatch",
                "stage": "task_status",
                "reason": "task is paused",
                "hold_kind": paused_hold_kind(task),
                "pause_reason": task.pause_reason.as_deref(),
                "status_label": task.status_label.as_deref(),
            })),
        }),
        status => Err(super::plan_rpc_err(PlanError::TransitionDenied {
            from: status.clone(),
            to: codeterm_service_core::plan::TaskStatus::Pending,
        })),
    }
}

fn paused_hold_kind(task: &TaskDispatchPreflightRead) -> &'static str {
    if task.status_label.as_deref() == Some("provider_exhausted") {
        "provider_exhausted"
    } else if task
        .pause_reason
        .as_deref()
        .is_some_and(|reason| reason.starts_with("gate:"))
    {
        "decision_gate"
    } else {
        "unclassified"
    }
}

/// Decide placement first — occupancy only applies to a checkout the task
/// will actually land on, never a pre-provisioning placeholder — codeterm#343.
#[cfg(test)]
pub(super) fn preflight_dispatch_placement(
    plan: &codeterm_service_core::plan::Plan,
    task_id: &str,
    spawn_cwd: &str,
    lane_protection: LaneProtection,
    will_provision_lane: bool,
    reported_executor_tab_statuses: &HashMap<(String, String), Option<String>>,
) -> Result<(), JsonRpcError> {
    if will_provision_lane {
        return Ok(());
    }
    preflight_checkout_occupancy(
        plan,
        task_id,
        spawn_cwd,
        lane_protection,
        reported_executor_tab_statuses,
    )
}

pub(super) fn selected_regression_dispatch_warning(
    store: &codeterm_service_core::service::plan::PlanStore,
    plan_id: &str,
) -> Result<Option<String>, JsonRpcError> {
    let input = store
        .regression_gate_projection(plan_id)
        .map_err(plan_rpc_err)?
        .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
    Ok(regression_dispatch_warning(regression_dispatch_decision(
        input,
    )))
}

pub(crate) fn regression_dispatch_warning(decision: RegressionDispatchDecision) -> Option<String> {
    let RegressionDispatchDecision::Advisory {
        regression_gates,
        retrospectives,
    } = decision
    else {
        return None;
    };
    let mut gates = regression_gates
        .iter()
        .chain(&retrospectives)
        .map(|gate| gate.key.as_str())
        .collect::<Vec<_>>();
    gates.sort_unstable();
    gates.dedup();
    Some(format!(
        "terminal-group gate advisory (dispatch allowed): {}",
        gates.join(", ")
    ))
}

pub(super) fn preflight_dispatch_checkout(
    store: &codeterm_service_core::service::plan::PlanStore,
    svc: &ServiceState,
    plan_id: &str,
    task_id: &str,
    spawn_cwd: &str,
    protection: LaneProtection,
    will_provision_lane: bool,
) -> Result<(), JsonRpcError> {
    if will_provision_lane || protection.is_waived() {
        return Ok(());
    }
    let live_tabs = svc
        .mux
        .read()
        .all_tabs()
        .map(|tab| tab.id.clone())
        .collect::<Vec<_>>();
    let occupant = store
        .dispatch_checkout_occupant_projection(plan_id, task_id, spawn_cwd, &live_tabs)
        .map_err(plan_rpc_err)?;
    lane_safety::checkout_occupancy_refusal(
        plan_id,
        task_id,
        spawn_cwd,
        occupant.as_ref().map(|occupant| {
            (
                occupant.task_id.as_str(),
                occupant.attempt_id.as_str(),
                occupant.executor_tab_id.as_deref(),
            )
        }),
    )?;
    Ok(())
}

#[cfg(test)]
pub(super) fn preflight_review_capacity(
    plan: &codeterm_service_core::plan::Plan,
    task: &codeterm_service_core::plan::Task,
) -> Result<(), JsonRpcError> {
    codeterm_service_core::service::plan::review_gate::validate_dispatch_review_capacity(plan, task)
        .map_err(plan_rpc_err)
}
