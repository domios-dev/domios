use super::SpawnAgentBody;
use codeterm_service_core::service::SpawnAgentParams;

pub(super) fn into_params(
    body: SpawnAgentBody,
    role_probe: SpawnAgentParams,
    ownership: Option<codeterm_types::pty::RemoteOwnership>,
) -> SpawnAgentParams {
    let starter_prompt =
        codeterm_service_core::service::starter_prompt_for_role(role_probe.role.as_deref());
    codeterm_service_core::service::SpawnAgentParams {
        context_files: None,
        neighbor_authority: None,
        context_skills: None,
        agent: body.agent,
        agent_name: body.agent_name,
        args: body.args,
        cwd: body.cwd,
        provisioned_cwd: None,
        title: body.title,
        title_provenance: Some(
            codeterm_service_core::service::SpawnIdentityProvenance::UserSupplied,
        ),
        description: body.description,
        task_description_managed: false,
        role: role_probe.role,
        role_profile: role_probe.role_profile,
        parent_tab_id: body.parent_tab_id,
        plan_id: body.plan_id,
        can_spawn: body.can_spawn,
        task: body.task,
        starter_prompt,
        task_id: body.task_id,
        task_launch_authority: None,
        plan_skill_manifest: None,
        plan_task: body.plan_task,
        authorized_gates: body.authorized_gates,
        execution_intent: body.execution_intent,
        sandbox: body.sandbox,
        provider_reserve_pct: body.provider_reserve_pct,
        workspace_id: body.workspace_id,
        force_tab: body.force_tab,
        existing_tab_id: None,
        skip_orchestrator_onboarding: None,
        report_target_tab_id: body.report_to.or(body.report_target_tab_id),
        report_target_mesh_peer: body.origin_peer,
        skip_permissions: None,
        permission_bypass_authorization: None,
        spawned_by_tab_id: body.spawned_by_tab_id,
        team_id: body.team_id,
        creator_tab_id: body.creator_tab_id,
        ownership,
        model: body.model,
        speed: body.speed,
        planned_model: None,
        planned_reasoning_effort: None,
        reasoning_effort: body.reasoning_effort,
        system_prompt: body.system_prompt,
        preset: body.preset,
        execution_mode: body.execution_mode,
        resume_session_id: None,
        fork_session: false,
        charter: body.charter,
        tick_interval_secs: body.tick_interval_secs,
        nudge_cooldown_secs: body.nudge_cooldown_secs,
        max_actions_per_tick: body.max_actions_per_tick,
        activate: body.activate,
        audience: body.audience,
        manager_bootstrap: None,
        user_created: false,
        structured_prompt_server: None,
        remote_task_binding: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_request_preserves_explicit_mode_environment_and_opaque_provider() {
        let body: SpawnAgentBody = serde_json::from_value(serde_json::json!({"agent":"opaque-provider","execution_mode":"opaque-cloud","preset":"opaque-environment","task":"opaque-task","speed":"opaque-tier"})).unwrap();
        let params = into_params(body, SpawnAgentParams::default(), None);
        assert_eq!(params.agent.as_deref(), Some("opaque-provider"));
        assert_eq!(params.speed.as_deref(), Some("opaque-tier"));
        assert_eq!(params.execution_mode.as_deref(), Some("opaque-cloud"));
        assert_eq!(params.preset.as_deref(), Some("opaque-environment"));
        assert_eq!(params.task.as_deref(), Some("opaque-task"));
        let body = serde_json::from_value(serde_json::json!({"agent":"opaque-provider"})).unwrap();
        assert!(into_params(body, SpawnAgentParams::default(), None)
            .execution_mode
            .is_none());
    }
}
