use super::*;

#[cfg(test)]
mod tests;

pub(super) fn try_task_bound_worker_transfer(
    service: &crate::service::ServiceContext,
    headers: &HeaderMap,
    worker_tab_id: &str,
    role: AgentRole,
    explicit_role_profile: Option<&str>,
    plan_id: Option<&str>,
    destination_tab_id: Option<&str>,
    can_spawn: Option<bool>,
) -> Result<Option<serde_json::Value>, String> {
    if role != AgentRole::Worker {
        return Ok(None);
    }
    let Some(destination_tab_id) = destination_tab_id else {
        return Ok(None);
    };
    if !service.task_bound_worker_transfer_candidate(worker_tab_id, destination_tab_id) {
        return Ok(None);
    }
    if plan_id.is_some() || can_spawn.is_some() {
        return Err("task-bound Worker transfer refuses plan binding or spawn changes".into());
    }
    let current_profile = service
        .mux
        .read()
        .get_tab(worker_tab_id)
        .and_then(|tab| tab.role_profile.clone());
    if explicit_role_profile.is_some_and(|profile| Some(profile) != current_profile.as_deref()) {
        return Err("task-bound Worker transfer preserves its current role profile".into());
    }
    let outcome = service.transfer_task_bound_worker(
        &crate::api::tab_routes::authenticated_plan_caller(headers),
        worker_tab_id,
        destination_tab_id,
    )?;
    Ok(Some(serde_json::json!({
        "status": outcome.status,
        "receipt_id": outcome.receipt_id,
        "worker_tab_id": outcome.worker_tab_id,
        "source_manager_tab_id": outcome.source_manager_tab_id,
        "destination_manager_tab_id": outcome.destination_manager_tab_id,
        "source_team_id": outcome.source_team_id,
        "destination_team_id": outcome.destination_team_id,
        "plan_id": outcome.plan_id,
        "task_id": outcome.task_id,
        "attempt_id": outcome.attempt_id,
    })))
}
