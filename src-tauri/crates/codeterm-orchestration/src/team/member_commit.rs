use super::*;
use codeterm_service_core::service::agent_transition::{
    plan_owner_request, TransitionFailurePoint,
};

#[allow(clippy::too_many_arguments)]
pub fn commit_team_member_prepared(
    svc: &ServiceContext,
    team_id: &str,
    caller: Option<&str>,
    tab_id: &str,
    role: &str,
    profile: &Option<String>,
    anchors: bool,
    now_ms: u64,
) -> Result<AddTeamMemberResult, String> {
    let captured = svc
        .mux
        .read()
        .get_tab(tab_id)
        .cloned()
        .ok_or("spawned Team member disappeared")?;
    let result = svc.transition_agent_prepared_with_at(
        now_ms,
        |mux, store| {
            if !member_context_matches(mux, tab_id, &captured) {
                return Err(
                    "spawned member context conflict: live identity or placement changed".into(),
                );
            }
            let mut result = staffing::commit_team_member_spawn_staged(
                mux, team_id, caller, tab_id, role, profile,
            )?;
            let Some((plan_id, previous)) = (if anchors {
                store
                    .team_owner_projection(team_id)
                    .map_err(|error| error.to_string())?
            } else {
                None
            }) else {
                return Ok((None, result));
            };
            let previous = previous.unwrap_or_default();
            if previous == tab_id
                || mux.get_tab(&previous).is_some_and(|tab| {
                    tab.anchors_team()
                        && tab.team_id.as_deref() == Some(team_id)
                        && crate::orchestrator_onboard::tab_is_agent_capable(tab)
                })
            {
                return Ok((None, result));
            }
            let reason = if previous.is_empty() {
                format!("Orchestrator recovery: Team {team_id} plan had no owner tab")
            } else {
                format!("Orchestrator recovery: owner tab {previous} is gone")
            };
            let request = plan_owner_request(
                codeterm_service_core::plan::OrchestratorRef::from_tab(tab_id),
                &plan_id,
                team_id,
                &reason,
                true,
                None,
                false,
            )?;
            result.plan_takeover = Some(staffing::PlanTakeover {
                plan_id,
                owner_tab_id: tab_id.into(),
                error: None,
            });
            Ok((Some(request), result))
        },
        TransitionFailurePoint::None,
    );
    match result {
        Ok((_, _, result)) => {
            codeterm_service_core::service::publish_mux_state_changed(&svc.events);
            Ok(result)
        }
        Err(error) => {
            let mut mux = svc.mux.write();
            if !member_context_matches(&mux, tab_id, &captured) {
                return Err(format!(
                    "{error}; spawned member cleanup conflict: live identity or placement changed"
                ));
            }
            mux.close_tab_with_reason(tab_id, TabCloseReason::SpawnRollback)
                .map_err(|rollback| {
                    format!("{error}; spawned member rollback failed: {rollback}")
                })?;
            Err(error)
        }
    }
}

fn member_context_matches(
    mux: &MultiplexerState,
    tab_id: &str,
    captured: &codeterm_service_core::multiplexer::types::Tab,
) -> bool {
    mux.get_tab(tab_id).is_some_and(|tab| {
        tab.lifecycle_epoch == captured.lifecycle_epoch
            && tab.lifecycle_operation_id == captured.lifecycle_operation_id
            && tab.agent_session_id == captured.agent_session_id
            && tab.session_binding_nonce == captured.session_binding_nonce
            && tab.pty_id == captured.pty_id
            && tab.role == captured.role
            && tab.role_profile == captured.role_profile
            && tab.team_id == captured.team_id
            && tab.parent_tab_id == captured.parent_tab_id
            && tab.task_binding == captured.task_binding
    })
}
