use super::*;

const TASK: &str = " \tfragment-97α\r\n$opaque `x` \\\" {task}\ntrailing\t ";
const PEER: &str = "peer-fragment-62β\n{opaque}";

#[test]
fn bound_planner_pty_bootstrap_preserves_opaque_task_for_both_delivery_modes() {
    for system_capable in [false, true] {
        let params = SpawnAgentParams {
            role: Some("Planner".into()),
            task: Some(TASK.into()),
            ..Default::default()
        };
        let prepared = spawn_launch_task_for_provider(
            &crate::agent::registry::AgentRegistry::new(),
            &params,
            system_capable,
        );
        let task = planner_startup_payload(system_capable, prepared, PEER);
        let task = team_bootstrap_payload(system_capable, task);
        assert_eq!(task.matches(TASK).count(), 1);
        assert_eq!(task.matches(PEER).count(), 1);
        if !system_capable {
            let input = crate::service::launch_delivery::prepare_spawn_input(
                None,
                params.role.as_deref(),
                params.task.as_deref(),
                crate::agent::provider::StarterPrompt::AssignedTask,
                None,
                str::to_string,
            );
            let spec = spawn_fallback_spec(
                &input,
                true,
                Some("short-bootstrap"),
                Some(&task),
                Some("opaque-starter"),
            )
            .unwrap();
            assert_eq!(spec.payload.matches(TASK).count(), 1);
            assert_eq!(spec.payload.matches(PEER).count(), 1);
            assert_eq!(spec.payload.matches("opaque-starter").count(), 1);
        }
    }
}

#[test]
fn bound_planner_chat_bootstrap_preserves_task_overlay_and_peer() {
    let (fixture, plan_id, _, owner) = crate::service::plan_authority::test_fixture::fixture();
    let mut params = SpawnAgentParams {
        role: Some("Manager".into()),
        role_profile: Some("planner".into()),
        plan_id: Some(plan_id),
        task: Some(TASK.into()),
        system_prompt: Some(PEER.into()),
        ..Default::default()
    };
    runtime_prompt::prepare_runtime_identity(&fixture.service, &mut params);
    let prompt = compose_chatbackend_system_prompt(&mut params).unwrap();
    assert_eq!(prompt.matches(TASK).count(), 1);
    assert_eq!(prompt.matches(PEER).count(), 1);
    assert!(prompt.contains(owner.tab_id.as_deref().unwrap()));
    let config = crate::service::spawn_params::chatbackend_spawn_config(
        Some(&prompt),
        None,
        None,
        None,
        None,
        None,
    );
    assert_eq!(config["systemPrompt"], prompt);
}
