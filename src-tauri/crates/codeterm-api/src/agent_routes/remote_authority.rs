use axum::http::StatusCode;
use codeterm_service_core::mesh::crown::AuthenticatedAgent;

use super::{SpawnAgentBody, SpawnRejection};

pub(super) fn prepare(
    agent: &AuthenticatedAgent,
    body: &mut SpawnAgentBody,
) -> Result<(), SpawnRejection> {
    if !agent.can_spawn {
        return Err(SpawnRejection::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "The originating agent cannot delegate".into(),
        ));
    }
    if body.parent_tab_id.is_some()
        || body.team_id.is_some()
        || body.plan_id.is_some()
        || body.plan_task.is_some()
        || body.task_id.is_some()
        || !body.authorized_gates.is_empty()
    {
        return Err(SpawnRejection::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Remote agents cannot claim local hierarchy or task authority".into(),
        ));
    }
    let role_type = body.role.clone().unwrap_or_else(|| "worker".into());
    let role = super::validate_spawn_role(None, Some(&role_type))
        .map_err(|(status, message)| SpawnRejection::new(status, "bad_request", message))?;
    if role.role.as_deref() != Some("Worker") {
        return Err(SpawnRejection::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Remote agents may delegate Workers".into(),
        ));
    }
    body.role = Some(role_type);
    body.can_spawn = Some(false);
    body.spawned_by_tab_id = None;
    body.creator_tab_id = None;
    body.report_to = Some(agent.tab_id.clone());
    body.report_target_tab_id = None;
    body.origin_peer = Some(agent.instance_id.clone());
    body.ownership = Some(codeterm_types::pty::RemoteOwnership {
        controller_title: agent.tab_id.clone(),
        controller_tab_id: Some(agent.tab_id.clone()),
        peer_id: agent.instance_id.clone(),
        peer_name: agent.instance_id.clone(),
        team: None,
    });
    Ok(())
}

#[cfg(test)]
mod tests;
