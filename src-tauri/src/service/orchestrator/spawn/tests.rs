use super::*;

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn sync_planner_spawn_resolves_plan_owner_before_provisioning() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut plan = crate::plan::Plan::new(
        "parent resolution fixture".into(),
        fixture.temp.path().display().to_string(),
        crate::plan::OrchestratorRef::from_tab("absent-owner-fixture"),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let before = fixture.service.mux.read().all_tabs().count();
    let result = fixture.service.spawn_agent_sync(SpawnAgentParams {
        role: Some("Planner".into()),
        plan_id: Some(plan.id.clone()),
        ..Default::default()
    });
    let Err(error) = result else {
        panic!("a missing Plan owner must refuse before spawn");
    };
    assert!(error.contains(&plan.id));
    assert!(error.contains("absent-owner-fixture"));
    assert_eq!(fixture.service.mux.read().all_tabs().count(), before);
}

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
async fn task_specific_empty_provider_command_fails_orchestrated_spawn_and_cleans_tab() {
    let mut mux = crate::multiplexer::state::MultiplexerState::empty();
    let workspace_id = mux.create_workspace("spawn rollback".into(), None).id;
    let parent_tab_id = mux.create_tab(&workspace_id).unwrap();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.get_tab_mut(&parent_tab_id).unwrap().agent_type = Some("supervisor".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_empty_launch_command_when_task_present(
            "task-sensitive-empty-launch",
        ),
    ));
    fixture
        .service
        .require_agent_provider_ready("task-sensitive-empty-launch")
        .expect("provider preflight without a task succeeds");
    let mut plan = fixture
        .service
        .plan_store
        .create_plan_for_team(
            "empty provider launch".into(),
            fixture.temp.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&parent_tab_id),
            "team-empty-launch".into(),
            vec![],
            1,
        )
        .unwrap();
    plan.mode = crate::plan::PlanMode::Approved;
    fixture.service.plan_store.save(&mut plan, 2).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            crate::service::plan::NewTask {
                name: "empty-launch".into(),
                handle: Some("empty-launch".into()),
                body: "Refuse a provider that cannot build a task launch.".into(),
                acceptance_criteria: vec!["No task attempt is bound.".into()],
                ..Default::default()
            },
            3,
        )
        .unwrap();
    fixture.service.ensure_agent_pad(&tab_id).unwrap();
    fixture
        .service
        .register_agent_startup_supervision(&parent_tab_id, &tab_id, "Worker");
    assert!(fixture.service.has_agent_startup_supervision(&tab_id));

    let result = fixture
        .service
        .spawn_agent(
            SpawnAgentParams {
                agent: Some("task-sensitive-empty-launch".into()),
                task: Some("assigned work".into()),
                role: Some("Worker".into()),
                parent_tab_id: Some(parent_tab_id),
                workspace_id: Some(workspace_id),
                existing_tab_id: Some(tab_id.clone()),
                plan_id: Some(plan.id.clone()),
                plan_task: Some("empty-launch".into()),
                execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
                provider_reserve_pct: None,
                ..Default::default()
            },
            None,
        )
        .await;

    let Err(error) = result else {
        panic!("a task-specific missing provider command must fail");
    };
    assert!(error.contains("did not resolve a launch command"), "{error}");
    let mux = fixture.service.mux.read();
    assert!(mux.get_tab(&tab_id).is_none());
    assert_eq!(mux.all_tabs().count(), 1);
    drop(mux);
    assert_eq!(
        crate::service::launch_session_detection::registered_launch_marker(&tab_id),
        None
    );
    assert!(!fixture.service.has_agent_startup_supervision(&tab_id));
    assert!(crate::service::agent_pad::AgentPad::open(
        &crate::paths::data_dir(),
        &tab_id
    )
    .is_err());

    let stored_plan = fixture.service.plan_store.get(&plan.id).unwrap();
    let task = stored_plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.status, crate::plan::TaskStatus::Pending);
    assert!(task.assigned_tab_id.is_none());
    assert!(task.execution_attempts.is_empty());
}

#[test]
fn effort_provenance_keeps_plan_and_request_distinct() {
    let planned = resolved_effort_provenance(Some("high".into()), Some("medium".into()));
    assert_eq!(planned.planned.as_deref(), Some("high"));
    assert_eq!(planned.requested.as_deref(), Some("medium"));
    assert_eq!(planned.resolved.as_deref(), Some("medium"));

    let ordinary = resolved_effort_provenance(None, Some("high".into()));
    assert_eq!(ordinary.planned, None);
    assert_eq!(ordinary.requested.as_deref(), Some("high"));
}

fn worker_params(spawned_by: Option<&str>) -> SpawnAgentParams {
    SpawnAgentParams {
        role: Some("Worker".into()),
        spawned_by_tab_id: spawned_by.map(str::to_string),
        ..Default::default()
    }
}

// Live bounce oracle (dev aa793130a, notification 958a78b0): POST
// /api/agents/spawn with audience=automation still minted a user bell.
#[test]
fn ordinary_pty_tab_creation_carries_description_and_audience_from_params() {
    use crate::multiplexer::types::TabAudience;
    let mut state = crate::multiplexer::state::MultiplexerState::empty();
    let ws_id = state.create_workspace("ws".to_string(), None).id;
    let tab_id = state.create_tab(&ws_id).expect("tab created");
    let params = SpawnAgentParams {
        agent: Some("claude".into()),
        audience: TabAudience::Automation,
        description: Some("Reviews plans and records task progress".into()),
        ..Default::default()
    };

    initialize_ordinary_pty_agent_tab(state.get_tab_mut(&tab_id).expect("tab"), &params);

    assert_eq!(
        state.get_tab(&tab_id).expect("tab").audience,
        TabAudience::Automation,
        "audience declared on SpawnAgentParams must survive into the ordinary PTY tab record"
    );
    assert_eq!(
        state.get_tab(&tab_id).expect("tab").description.as_deref(),
        Some("Reviews plans and records task progress")
    );
}

// Negative control: an undeclared spawn stays user-facing on the same path.
#[test]
fn ordinary_pty_tab_creation_defaults_to_user_audience() {
    use crate::multiplexer::types::TabAudience;
    let mut state = crate::multiplexer::state::MultiplexerState::empty();
    let ws_id = state.create_workspace("ws".to_string(), None).id;
    let tab_id = state.create_tab(&ws_id).expect("tab created");
    let params = SpawnAgentParams {
        agent: Some("claude".into()),
        ..Default::default()
    };

    initialize_ordinary_pty_agent_tab(state.get_tab_mut(&tab_id).expect("tab"), &params);

    assert_eq!(
        state.get_tab(&tab_id).expect("tab").audience,
        TabAudience::User,
        "an undeclared spawn keeps the user audience and still bells"
    );
}

#[test]
fn worker_without_parent_defaults_to_caller_and_reports_to_parent() {
    let mut p = worker_params(Some("caller-tab"));
    apply_worker_hierarchy_defaults(&mut p);
    assert_eq!(p.parent_tab_id.as_deref(), Some("caller-tab"));
    assert_eq!(p.report_target_tab_id.as_deref(), Some("caller-tab"));
}

#[test]
fn explicit_parent_and_report_target_are_untouched() {
    let mut p = worker_params(Some("caller-tab"));
    p.parent_tab_id = Some("boss".into());
    p.report_target_tab_id = Some("boss".into());
    apply_worker_hierarchy_defaults(&mut p);
    assert_eq!(p.parent_tab_id.as_deref(), Some("boss"));
    assert_eq!(p.report_target_tab_id.as_deref(), Some("boss"));
}

#[test]
fn cross_mesh_and_non_worker_are_skipped() {
    let mut mesh = worker_params(Some("caller"));
    mesh.report_target_mesh_peer = Some("peer".into());
    apply_worker_hierarchy_defaults(&mut mesh);
    assert_eq!(mesh.parent_tab_id, None);

    let mut orch = SpawnAgentParams {
        role: Some("Orchestrator".into()),
        spawned_by_tab_id: Some("caller".into()),
        ..Default::default()
    };
    apply_worker_hierarchy_defaults(&mut orch);
    assert_eq!(orch.parent_tab_id, None);
}

#[test]
fn chatbackend_worker_receives_identity_context_contract_and_assignment_once() {
    let mut params = SpawnAgentParams {
        role: Some("Worker".into()),
        task: Some("repair transport".into()),
        ..Default::default()
    };

    let prompt = compose_chatbackend_system_prompt(&mut params).unwrap();

    assert!(prompt.starts_with("You are a CodeTerm Worker."));
    assert_eq!(
        prompt
            .matches(crate::prompts::WORKER_CODETERM_CONTEXT.trim())
            .count(),
        1
    );
    assert_eq!(
        prompt
            .matches(crate::prompts::AGENT_CODETERM_PRIMER.trim())
            .count(),
        1
    );
    assert_eq!(
        prompt.matches(crate::prompts::WORKER_SUFFIX.trim()).count(),
        1
    );
    assert_eq!(prompt.matches("repair transport").count(), 1);
    assert_eq!(params.task, None);
}

fn team_bound_bootstrap(
    coordination: Option<&str>,
) -> crate::service::role_identity::ManagerBootstrapInputs {
    crate::service::role_identity::ManagerBootstrapInputs {
        role: "Orchestrator".into(),
        role_kind: "orchestrator".into(),
        team_id: "team_abcd1234".into(),
        team_name: "Halley".into(),
        plan_id: crate::service::spawn_factory::UNBOUND_PLAN.into(),
        project: "CodeTerm".into(),
        coordination: coordination.map(str::to_string),
    }
}

#[test]
fn system_capable_idle_manager_bootstrap_adds_no_user_starter_turn() {
    let coordination = "Your Planner counterpart is Euler (tab planner-1).";
    let mut params = SpawnAgentParams {
        manager_bootstrap: Some(team_bound_bootstrap(Some(coordination))),
        ..Default::default()
    };

    let bootstrap = prepare_manager_bootstrap(&mut params, true).unwrap();

    assert_eq!(
        params.starter_prompt,
        crate::agent::provider::StarterPrompt::NoStarter,
        "a system-prompt-capable idle Manager never repeats its bootstrap as a user turn"
    );
    assert_eq!(params.task.as_deref(), Some(bootstrap.as_str()));
    assert_eq!(bootstrap.matches(coordination).count(), 1);
    assert_eq!(
        team_bootstrap_payload(true, bootstrap.clone()),
        bootstrap,
        "the system channel carries the contract unwrapped"
    );
}

#[test]
fn external_idle_manager_bootstrap_keeps_the_team_starter_turn() {
    let coordination = "Your Planner counterpart is Euler (tab planner-1).";
    let mut params = SpawnAgentParams {
        manager_bootstrap: Some(team_bound_bootstrap(Some(coordination))),
        ..Default::default()
    };

    let bootstrap = prepare_manager_bootstrap(&mut params, false).unwrap();

    assert_eq!(
        params.starter_prompt,
        crate::agent::provider::StarterPrompt::TeamBootstrap,
        "a provider without system prompts still needs the CodeTerm envelope turn"
    );
    assert_eq!(params.task.as_deref(), Some(bootstrap.as_str()));
    assert!(team_bootstrap_payload(false, bootstrap).contains("codeterm:system"));
}

#[test]
fn idle_manager_bootstrap_carries_the_bound_team_and_an_unbound_plan() {
    let mut params = SpawnAgentParams {
        manager_bootstrap: Some(team_bound_bootstrap(None)),
        ..Default::default()
    };

    let bootstrap = prepare_manager_bootstrap(&mut params, true).unwrap();

    assert!(bootstrap.contains("team_abcd1234"));
    assert!(bootstrap.contains("Halley"));
    assert!(bootstrap.contains(crate::service::spawn_factory::UNBOUND_PLAN));
    assert!(!bootstrap.contains(crate::service::spawn_factory::UNBOUND_TEAM));
}

#[test]
fn bootstrap_bound_team_id_returns_real_id_and_rejects_placeholders() {
    let bound = team_bound_bootstrap(None);
    assert_eq!(bootstrap_bound_team_id(Some(&bound)), Some("team_abcd1234"));

    assert_eq!(bootstrap_bound_team_id(None), None);

    let mut unbound = team_bound_bootstrap(None);
    unbound.team_id = crate::service::spawn_factory::UNBOUND_TEAM.into();
    assert_eq!(bootstrap_bound_team_id(Some(&unbound)), None);

    let mut blank = team_bound_bootstrap(None);
    blank.team_id = "   ".into();
    assert_eq!(bootstrap_bound_team_id(Some(&blank)), None);
}

#[test]
fn assigned_manager_bootstrap_keeps_the_assigned_task_starter_in_both_modes() {
    for system_prompt_capable in [true, false] {
        let mut params = SpawnAgentParams {
            task: Some("Audit the delivery path.".into()),
            manager_bootstrap: Some(team_bound_bootstrap(None)),
            ..Default::default()
        };

        let bootstrap = prepare_manager_bootstrap(&mut params, system_prompt_capable).unwrap();
        let expected = format!("{bootstrap}\n\nAudit the delivery path.");
        assert_eq!(
            params.starter_prompt,
            crate::agent::provider::StarterPrompt::AssignedTask
        );
        assert_eq!(params.task.as_deref(), Some(expected.as_str()));
    }
}

#[test]
fn supervised_spawn_replaces_the_existing_first_turn_with_startup_handshake() {
    let mut params = SpawnAgentParams {
        starter_prompt: crate::agent::provider::StarterPrompt::AssignedTask,
        ..Default::default()
    };
    apply_startup_handshake(&mut params, true);
    assert_eq!(
        params.starter_prompt,
        crate::agent::provider::StarterPrompt::StartupHandshake
    );

    apply_startup_handshake(&mut params, false);
    assert_eq!(
        params.starter_prompt,
        crate::agent::provider::StarterPrompt::StartupHandshake
    );
}

#[test]
fn native_planner_startup_keeps_manager_contract_and_onboarding() {
    let payload = planner_startup_payload(true, Some("system contract".into()), "planner startup");

    assert!(payload.starts_with("system contract"));
    assert!(payload.ends_with("planner startup"));
    assert!(!payload.contains("codeterm:system"));
}

#[test]
fn team_bootstrap_marks_only_the_fallback_transport() {
    let system_prompt = team_bootstrap_payload(true, "system contract".into());
    assert_eq!(system_prompt, "system contract");
    assert!(!system_prompt.contains("codeterm:system"));

    let fallback = team_bootstrap_payload(false, "fallback contract".into());
    assert_eq!(
        fallback,
        "-=-codeterm:system context=team-bootstrap-=-\nfallback contract"
    );
    assert!(!fallback.contains("tab="));
    assert!(!fallback.contains("[to reply"));
}

#[test]
fn roleless_agent_starts_without_a_visible_turn() {
    let mut params = SpawnAgentParams::default();

    prepare_standard_starter_prompt(&mut params);

    assert_eq!(
        params.starter_prompt,
        crate::agent::provider::StarterPrompt::NoStarter
    );
}

#[test]
fn roleless_task_is_system_context_without_a_visible_turn() {
    let mut params = SpawnAgentParams {
        task: Some("Inspect the launch path.".into()),
        ..Default::default()
    };

    prepare_standard_starter_prompt(&mut params);

    assert_eq!(
        params.starter_prompt,
        crate::agent::provider::StarterPrompt::NoStarter
    );
}

#[test]
fn role_assignment_keeps_the_assigned_task_turn() {
    let mut params = SpawnAgentParams {
        task: Some("Inspect the launch path.".into()),
        role: Some("Worker".into()),
        ..Default::default()
    };

    prepare_standard_starter_prompt(&mut params);

    assert_eq!(
        params.starter_prompt,
        crate::agent::provider::StarterPrompt::AssignedTask
    );
}

// Pin the daemon cwd when both inputs are absent; `None` desyncs the tab
// from the inherited PTY cwd and points chat at the wrong session store.
#[test]
fn no_cwd_and_no_workspace_pins_the_daemon_cwd() {
    let (resolved, warning) = ServiceContext::resolve_agent_cwd(&None, &None);
    assert!(warning.is_none());
    let expected = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(String::from));
    assert_eq!(resolved, expected);
    assert!(
        resolved.as_deref().is_none_or(|d| !d.is_empty()),
        "resolved cwd must never be an empty string"
    );
}
