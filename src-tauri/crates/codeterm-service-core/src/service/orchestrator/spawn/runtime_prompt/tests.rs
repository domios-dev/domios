use super::*;
use crate::service::role_preset::{FragmentSource, PresetFragment};

fn opaque_fragments() -> RuntimeFragments<'static> {
    RuntimeFragments {
        primer: "opaque-primer",
        plugins: Some("opaque-plugins"),
        authority: "opaque-authority",
        context: "opaque-context",
        pad: "opaque-pad",
        rendering: "opaque-rendering",
    }
}

#[test]
fn every_runtime_role_composes_opaque_identity_after_primer_without_residue() {
    for (role, kind) in [
        (AgentRole::Manager, "orchestrator"),
        (AgentRole::Manager, "planner"),
        (AgentRole::Manager, "cartographer"),
        (AgentRole::Manager, "watcher"),
        (AgentRole::Manager, "general_agent"),
        (AgentRole::Manager, "opaque-manager-kind"),
        (AgentRole::Worker, "worker"),
        (AgentRole::Worker, "opaque-worker-kind"),
    ] {
        let contract = format!("opaque-contract-{kind}");
        let preset = RolePreset {
            role,
            kind_id: kind.into(),
            label: format!("opaque-label-{kind}"),
            fragments: vec![PresetFragment {
                source: FragmentSource::Builtin,
                text: contract.clone(),
            }],
        };
        let params = SpawnAgentParams {
            task: Some("opaque-assignment".into()),
            system_prompt: Some("opaque-overlay".into()),
            ..Default::default()
        };
        let prompt = compose_with(&params, Some(&preset), None, opaque_fragments());
        for fragment in [
            "opaque-primer",
            "opaque-plugins",
            "opaque-authority",
            &contract,
            "opaque-context",
            "opaque-pad",
            "opaque-rendering",
            "opaque-assignment",
            "opaque-overlay",
        ] {
            assert_eq!(prompt.matches(fragment).count(), 1, "{kind}: {fragment}");
        }
        assert!(prompt.find("opaque-primer") < prompt.find(&contract));
        assert!(prompt.find(&contract) < prompt.find("opaque-assignment"));
        assert!(!prompt.contains('{'));
        assert!(!prompt.contains('}'));
    }
}

#[test]
fn manager_bootstrap_and_overlay_are_each_delivered_once_with_assignment() {
    let mut params = SpawnAgentParams {
        role: Some("Manager".into()),
        role_profile: Some("planner".into()),
        task: Some("opaque-assignment".into()),
        system_prompt: Some("opaque-overlay".into()),
        ..Default::default()
    };
    let prompt = compose_with(&params, None, Some("opaque-bootstrap"), opaque_fragments());
    assert_eq!(prompt.matches("opaque-bootstrap").count(), 1);
    assert_eq!(prompt.matches("opaque-assignment").count(), 1);
    assert!(prompt.find("opaque-primer") < prompt.find("opaque-bootstrap"));
    assert!(prompt.find("opaque-bootstrap") < prompt.find("opaque-assignment"));
    let prompt = compose_runtime_system_prompt(&mut params).unwrap();
    assert_eq!(prompt.matches("opaque-assignment").count(), 1);
    assert_eq!(params.task.as_deref(), Some("opaque-assignment"));
    params.system_prompt = Some(prompt.clone());
    assert_eq!(compose_runtime_system_prompt(&mut params), Some(prompt));
}

#[test]
fn worker_runtime_keeps_personality_executor_guidance_and_brief() {
    let identity = crate::service::plan::worker_identity::compose_worker_identity_contract(
        Some("opaque-personality"),
        Some("opaque-guidance"),
        Some("opaque-executor"),
        "opaque-task-name",
    );
    let mut params = SpawnAgentParams {
        role: Some("Worker".into()),
        task: Some(
            crate::service::plan::worker_identity::attach_worker_identity(
                &identity,
                "opaque-brief",
            ),
        ),
        ..Default::default()
    };
    let prompt = compose_runtime_system_prompt(&mut params).unwrap();
    for fragment in [
        "opaque-personality",
        "opaque-executor",
        "opaque-guidance",
        "opaque-brief",
    ] {
        assert_eq!(prompt.matches(fragment).count(), 1);
    }
    assert!(
        prompt.find(crate::prompts::AGENT_CODETERM_PRIMER.trim())
            < prompt.find("opaque-personality")
    );
    assert!(params.task.unwrap().contains("opaque-brief"));
}

#[test]
fn taskless_runtime_preserves_idle_kickoff_parity() {
    let mut params = SpawnAgentParams {
        role: Some("Worker".into()),
        ..Default::default()
    };
    assert!(compose_runtime_system_prompt(&mut params)
        .unwrap()
        .contains(ROLE_AUTHORITY));
    assert!(params.task.is_none());
}

#[test]
fn runtime_planner_spawn_config_carries_assignment_and_plan_identity() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.create_workspace("opaque-project".into(), None);
    let owner = mux.create_tab(&workspace.id).unwrap();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let plan = fixture
        .service
        .plan_store
        .create_plan_for_team(
            "opaque-plan-title".into(),
            fixture.temp.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&owner),
            "opaque-team".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut params = SpawnAgentParams {
        agent: Some("runtime-fixture".into()),
        role: Some("Manager".into()),
        role_profile: Some("planner".into()),
        plan_id: Some(plan.id.clone()),
        task: Some("opaque-assignment".into()),
        ..Default::default()
    };
    prepare_runtime_identity(&fixture.service, &mut params);
    params.system_prompt = compose_runtime_system_prompt(&mut params);
    let registry =
        crate::service::spawn_capability::test_backend::registry_with_runtime("runtime-fixture");
    let spec =
        crate::service::spawn::SpawnTabSpec::from_agent_params_with_chatbackends(&registry, params);
    let crate::service::spawn::TabCommand::ChatBackend { config, .. } = &spec.command else {
        panic!("runtime route absent");
    };
    let prompt = config["systemPrompt"].as_str().unwrap();
    assert_eq!(prompt.matches("opaque-assignment").count(), 1);
    assert!(prompt.contains(&plan.id));
    assert!(prompt.contains("opaque-team"));
    assert!(prompt.contains(crate::prompts::PLANNER_DOC.trim()));
    assert!(prompt.contains(ROLE_AUTHORITY));
    let kickoff = spec
        .chatbackend_launch
        .as_ref()
        .unwrap()
        .agent
        .task
        .as_deref()
        .unwrap();
    assert_eq!(kickoff.matches("opaque-assignment").count(), 1);
    assert!(kickoff.contains(&owner));
}

#[test]
fn roleless_system_overlay_cannot_supply_a_different_tab_role() {
    let mut params = SpawnAgentParams {
        system_prompt: Some("opaque-project-overlay".into()),
        ..Default::default()
    };
    let prompt = compose_runtime_system_prompt(&mut params).unwrap();
    assert_eq!(prompt.matches("opaque-project-overlay").count(), 1);
    assert_eq!(prompt.matches(ROLE_AUTHORITY).count(), 1);
    assert!(params.task.is_none());
}

#[test]
fn restored_runtime_identity_is_persisted_once_without_fabricating_kickoff() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.create_workspace("opaque-workspace".into(), None);
    let tab_id = mux.create_tab(&workspace.id).unwrap();
    let target = mux.get_tab_mut(&tab_id).unwrap();
    target.agent_type = Some("runtime-fixture".into());
    target.role = Some(AgentRole::Manager);
    target.role_profile = Some("planner".into());
    target.session_binding_nonce = Some("opaque-nonce".into());
    target.agent_session_id = Some("opaque-thread".into());
    target.restorable_launch.identity_instructions = Some("opaque-saved-identity".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    ensure_tab_runtime_prompt(&fixture.service, &tab_id).unwrap();
    let first = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    let prompt = first.agent_runtime_config.as_ref().unwrap()["systemPrompt"]
        .as_str()
        .unwrap();
    assert_eq!(prompt.matches("opaque-saved-identity").count(), 0);
    assert_eq!(
        prompt.matches(crate::prompts::PLANNER_DOC.trim()).count(),
        1
    );
    assert!(first.pending_delivery.is_none());
    assert_eq!(first.agent_session_id.as_deref(), Some("opaque-thread"));
    ensure_tab_runtime_prompt(&fixture.service, &tab_id).unwrap();
    let second = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(first.agent_runtime_config, second.agent_runtime_config);
    assert!(second.pending_delivery.is_none());
}

#[test]
fn restored_runtime_worker_keeps_its_saved_persona_once() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.create_workspace("opaque-workspace".into(), None);
    let tab_id = mux.create_tab(&workspace.id).unwrap();
    let target = mux.get_tab_mut(&tab_id).unwrap();
    target.agent_type = Some("runtime-fixture".into());
    target.role = Some(AgentRole::Worker);
    target.session_binding_nonce = Some("opaque-nonce".into());
    target.restorable_launch.identity_instructions = Some("opaque-worker-persona".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    ensure_tab_runtime_prompt(&fixture.service, &tab_id).unwrap();
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    let prompt = tab.agent_runtime_config.as_ref().unwrap()["systemPrompt"]
        .as_str()
        .unwrap();
    assert_eq!(prompt.matches("opaque-worker-persona").count(), 1);
    assert_eq!(prompt.matches(ROLE_AUTHORITY).count(), 1);
}
