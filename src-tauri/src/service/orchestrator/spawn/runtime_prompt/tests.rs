use super::*;

#[test]
fn manager_runtime_keeps_identity_and_binding_fragments_separate_from_user_task() {
    for (role, kind) in [("Orchestrator", "orchestrator"), ("Planner", "planner")] {
        let manager = crate::service::role_identity::ManagerBootstrapInputs {
            role: role.into(),
            role_kind: kind.into(),
            team_id: "opaque-team".into(),
            team_name: "opaque-name".into(),
            plan_id: "opaque-plan".into(),
            project: "opaque-project".into(),
            coordination: Some("opaque-counterpart-fragment".into()),
        };
        let bootstrap = manager.compose();
        let mut params = SpawnAgentParams {
            role: Some(role.into()),
            role_profile: Some(kind.into()),
            task: Some("opaque-user-assignment".into()),
            system_prompt: Some("opaque-developer-overlay".into()),
            manager_bootstrap: Some(manager),
            plan_id: Some("opaque-plan".into()),
            team_id: Some("opaque-team".into()),
            ..Default::default()
        };
        let prompt = compose_runtime_system_prompt(&mut params).unwrap();
        assert_eq!(prompt.matches(&bootstrap).count(), 1);
        assert_eq!(prompt.matches("opaque-developer-overlay").count(), 1);
        assert!(!prompt.contains("opaque-user-assignment"));
        assert_eq!(params.task.as_deref(), Some("opaque-user-assignment"));
        assert_eq!(params.plan_id.as_deref(), Some("opaque-plan"));
        assert_eq!(params.team_id.as_deref(), Some("opaque-team"));
        assert!(params.system_prompt.is_none());
    }
}

#[test]
fn worker_runtime_does_not_consume_or_repeat_the_initial_task() {
    let mut params = SpawnAgentParams {
        role: Some("Worker".into()),
        task: Some("opaque-worker-user-task".into()),
        ..Default::default()
    };
    let prompt = compose_runtime_system_prompt(&mut params).unwrap();
    assert!(!prompt.contains("opaque-worker-user-task"));
    assert_eq!(params.task.as_deref(), Some("opaque-worker-user-task"));
    assert_eq!(
        Some(prompt),
        crate::service::orchestrator::prompt_compose::restorable_identity_task(
            params.task.as_deref(),
            params.role.as_deref(),
            None,
        )
    );
}

#[test]
fn idle_runtime_role_bootstrap_does_not_create_a_user_task() {
    let mut params = SpawnAgentParams {
        role: Some("Planner".into()),
        role_profile: Some("planner".into()),
        ..Default::default()
    };
    assert!(compose_runtime_system_prompt(&mut params).is_some());
    assert!(params.task.is_none());
}
