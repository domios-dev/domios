use super::*;
use codeterm_types::team::general_agent_membership::GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION;

use crate::plan::PlannerRef;
use crate::service::plan::PlanStore;

fn store_for_test() -> (tempfile::TempDir, PlanStore) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    (root, store)
}

#[test]
fn pair_spawn_creates_no_plan_until_first_goal() {
    let (_root, store) = store_for_test();
    assert!(
        store.list_index().is_empty(),
        "pair spawn must not create a placeholder plan"
    );

    let plan = store
        .create_plan_for_team(
            "First goal".into(),
            _root.path().display().to_string(),
            OrchestratorRef::from_tab("orch-1"),
            "team-orch-1".into(),
            Vec::new(),
            1,
        )
        .unwrap();
    store
        .set_planner_ref(
            &plan.id,
            Some(PlannerRef::Tab {
                tab_id: "planner-1".into(),
                session_id: None,
                title: None,
            }),
            3,
        )
        .unwrap();

    let plans = store.list_index();
    assert_eq!(plans.len(), 1, "first goal creates exactly one plan");
    let bound = store.get(&plan.id).unwrap();
    assert_eq!(bound.owner.tab_id.as_deref(), Some("orch-1"));
    assert_eq!(
        bound.planner,
        Some(PlannerRef::Tab {
            tab_id: "planner-1".into(),
            session_id: None,
            title: None,
        })
    );
}

#[test]
fn pair_retry_command_preserves_provider_models_and_workspace() {
    let cmd = pair_retry_command("ws-1", "claude", Some("opus"), "codex", Some("gpt-5"));
    assert_eq!(
        cmd,
        "codeterm team pair --orchestrator claude:opus --planner codex:gpt-5 --workspace ws-1"
    );
}

#[test]
fn pair_without_model_does_not_promote_provider_defaults_to_explicit_requests() {
    assert_eq!(explicit_pair_model(None), None);
    assert_eq!(explicit_pair_model(Some("  ".into())), None);
    assert_eq!(
        explicit_pair_model(Some("gpt-6-sol".into())).as_deref(),
        Some("gpt-6-sol")
    );
}

#[test]
fn planner_name_is_distinct_from_orchestrator_and_never_the_role_word() {
    for _ in 0..64 {
        let name = derive_planner_name(["Kepler", "Main"], "Kepler");
        assert_ne!(
            name, "Kepler",
            "planner never copies the orchestrator's name"
        );
        assert_ne!(name, "Planner", "planner is never the literal role word");
        assert!(
            !name.to_ascii_lowercase().starts_with("planner"),
            "planner name never even STARTS with the role word (got '{name}')"
        );
        assert!(!name.trim().is_empty(), "planner always gets a real name");
    }
}

#[test]
fn planner_name_avoids_all_existing_workspace_titles() {
    let existing = ["Euler", "Noether", "Bohr"];
    for _ in 0..64 {
        let name = derive_planner_name(existing, "Euler");
        assert!(
            !existing.contains(&name.as_str()),
            "planner name '{name}' collided with an existing workspace tab"
        );
        assert_ne!(name, "Planner");
    }
}

#[test]
fn pair_tabs_are_reserved_before_either_agent_launches() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let reserved = reserve_pair_tabs(
        &mut mux,
        &workspace_id,
        None,
        false,
        &CorrectiveShape::default(),
    )
    .unwrap();

    assert_ne!(reserved.orchestrator_tab_id, reserved.planner_tab_id);
    assert!(mux.get_tab(&reserved.orchestrator_tab_id).is_some());
    assert!(mux.get_tab(&reserved.planner_tab_id).is_some());
    for tab_id in [&reserved.orchestrator_tab_id, &reserved.planner_tab_id] {
        let tab = mux.get_tab(tab_id).unwrap();
        assert!(tab.agent_booting, "reserved Tab must announce booting");
        assert_eq!(
            tab.boot_stage,
            Some(codeterm_types::pty::BootStage::PtyStarting),
            "reserved Tab must never expose a blank chat before launch"
        );
    }
    assert_eq!(
        mux.display_label_for_tab(&reserved.planner_tab_id),
        reserved.planner_name
    );
    assert!(mux
        .claim_existing_tab(&workspace_id, &reserved.orchestrator_tab_id)
        .is_ok());
    assert!(mux
        .claim_existing_tab(&workspace_id, &reserved.planner_tab_id)
        .is_ok());
}

fn reserved_pair() -> (
    crate::multiplexer::state::MultiplexerState,
    String,
    ReservedPairTabs,
) {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let reserved = reserve_pair_tabs(
        &mut mux,
        &workspace_id,
        None,
        false,
        &CorrectiveShape::default(),
    )
    .unwrap();
    (mux, workspace_id, reserved)
}

#[test]
fn pair_reserves_one_shared_team_before_either_agent_launches() {
    let (mux, _, reserved) = reserved_pair();

    assert!(reserved.team_minted, "a fresh pair mints its own Team");
    assert!(mux.get_team(&reserved.team.id).is_some());
    for tab_id in [&reserved.orchestrator_tab_id, &reserved.planner_tab_id] {
        assert_eq!(
            mux.get_tab(tab_id).unwrap().team_id.as_deref(),
            Some(reserved.team.id.as_str()),
            "both reserved Tabs carry the same Team before either launch"
        );
    }
    assert_eq!(
        reserved.team.name,
        crate::service::spawn_factory::UNNAMED_TEAM
    );
}

#[test]
fn both_pair_startup_identities_name_the_reserved_team_and_no_plan() {
    let (_mux, _, reserved) = reserved_pair();

    for (role, role_kind) in [("Orchestrator", ORCHESTRATOR), ("Planner", PLANNER)] {
        let identity = crate::service::spawn_factory::forming_identity_in_team(
            role,
            role_kind,
            "CodeTerm".into(),
            &reserved.team,
        );
        assert_eq!(identity.team_id, reserved.team.id);
        assert_eq!(
            identity.plan_id,
            crate::service::spawn_factory::UNBOUND_PLAN,
            "the pair Plan stays unbound until the first goal"
        );
        let rendered = identity.compose();
        assert!(rendered.contains(&reserved.team.id));
        assert!(!rendered.contains(crate::service::spawn_factory::UNBOUND_TEAM));
    }
}

#[test]
fn failed_pair_launch_releases_a_newly_minted_team() {
    let (mut mux, _, reserved) = reserved_pair();

    release_minted_pair_team(&mut mux, &reserved);

    assert!(mux.get_team(&reserved.team.id).is_none());
    for tab_id in [&reserved.orchestrator_tab_id, &reserved.planner_tab_id] {
        assert_eq!(mux.get_tab(tab_id).unwrap().team_id, None);
    }
}

#[test]
fn pair_startup_lease_covers_only_a_minted_team() {
    let (mut mux, workspace_id, reserved) = reserved_pair();

    let lease = pair_startup_lease("creator", &reserved).expect("a minted pair Team is leased");
    assert_eq!(lease.team_id, reserved.team.id);
    assert_eq!(lease.creator_tab_id, "creator");
    assert_eq!(
        lease.member_tab_ids,
        vec![
            reserved.orchestrator_tab_id.clone(),
            reserved.planner_tab_id.clone()
        ],
        "the lease owns exactly the two reserved launch attempts"
    );

    let claimed = mux.create_tab(&workspace_id).unwrap();
    mux.resolve_or_mint_team_for_parent(&claimed).unwrap();
    let adopted = reserve_pair_tabs(
        &mut mux,
        &workspace_id,
        Some(&claimed),
        false,
        &CorrectiveShape::default(),
    )
    .unwrap();

    assert!(
        pair_startup_lease("creator", &adopted).is_none(),
        "an adopted Team is never lifecycle-owned"
    );
}

#[test]
fn failed_pair_launch_keeps_a_team_the_claimed_tab_already_had() {
    let (mut mux, workspace_id, _) = reserved_pair();
    let claimed = mux.create_tab(&workspace_id).unwrap();
    let adopted_team_id = mux
        .resolve_or_mint_team_for_parent(&claimed)
        .unwrap()
        .team_id;

    let reserved = reserve_pair_tabs(
        &mut mux,
        &workspace_id,
        Some(&claimed),
        false,
        &CorrectiveShape::default(),
    )
    .unwrap();
    assert!(!reserved.team_minted);
    assert_eq!(reserved.team.id, adopted_team_id);

    release_minted_pair_team(&mut mux, &reserved);

    assert!(
        mux.get_team(&adopted_team_id).is_some(),
        "a Team the pair only adopted outlives a failed launch"
    );
    assert_eq!(
        mux.get_tab(&claimed).unwrap().team_id.as_deref(),
        Some(adopted_team_id.as_str())
    );
}

// ── General Agent membership guard ──────────────────────────────

fn general_agent_in_pair_workspace(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    workspace_id: &str,
) -> String {
    let tab_id = mux.create_tab(workspace_id).unwrap();
    crate::service::general_agent::identity::apply_general_agent_kind(
        mux.get_tab_mut(&tab_id).unwrap(),
    );
    tab_id
}

#[test]
fn general_agent_team_guard_refuses_self_pairing_and_reserves_nothing() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let general = general_agent_in_pair_workspace(&mut mux, &workspace_id);
    let teams_before = mux.team_count();
    let tabs_before = mux.snapshot().tabs.len();

    let error = reserve_pair_tabs(
        &mut mux,
        &workspace_id,
        Some(&general),
        false,
        &CorrectiveShape::default(),
    )
    .expect_err("the General Agent may not claim the Orchestrator seat of a pair");

    assert!(
        error.contains(GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION),
        "refusal must carry the stable violation id, got: {error}"
    );
    assert_eq!(mux.team_count(), teams_before, "no Team may be minted");
    assert_eq!(mux.snapshot().tabs.len(), tabs_before, "no Tab reserved");
    let tab = mux.get_tab(&general).unwrap();
    assert_eq!(tab.team_id, None, "the General Agent stays outside Teams");
    assert!(!tab.agent_booting, "a refused claim starts no boot");
}

#[test]
fn general_agent_team_guard_still_lets_the_general_agent_pair_for_other_agents() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let general = general_agent_in_pair_workspace(&mut mux, &workspace_id);
    let teams_before = mux.team_count();

    let reserved = reserve_pair_tabs(
        &mut mux,
        &workspace_id,
        None,
        false,
        &CorrectiveShape::in_workspace(Some(&workspace_id)),
    )
    .expect("creating a pair for OTHER agents is the General Agent's job");

    assert_eq!(mux.team_count(), teams_before + 1);
    assert!(reserved.team_minted);
    for tab_id in [&reserved.orchestrator_tab_id, &reserved.planner_tab_id] {
        assert_eq!(
            mux.get_tab(tab_id).unwrap().team_id.as_deref(),
            Some(reserved.team.id.as_str())
        );
    }
    let general_tab = mux.get_tab(&general).unwrap();
    assert_eq!(
        general_tab.team_id, None,
        "the pair belongs to its Orchestrator, never to its creator"
    );
}

#[test]
fn general_agent_is_not_misclassified_as_a_pair_creation_leaf() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let general = general_agent_in_pair_workspace(&mut mux, &workspace_id);
    assert!(!is_pair_creation_leaf(mux.get_tab(&general).unwrap()));

    let manager = mux.create_tab(&workspace_id).unwrap();
    let manager_tab = mux.get_tab_mut(&manager).unwrap();
    manager_tab.role = Some(crate::multiplexer::types::AgentRole::Manager);
    manager_tab.role_profile = Some(ORCHESTRATOR.to_string());
    assert!(is_pair_creation_leaf(mux.get_tab(&manager).unwrap()));
}

// ── codeterm#400: leaf pair-creation gate ───────────────────────

#[test]
fn leaf_without_declaration_is_refused_and_names_the_flag() {
    let refusal =
        pair_creation_refusal_for_leaf(true, false).expect("an undeclared leaf must be refused");
    assert!(
        refusal.contains("--acknowledge-leaf"),
        "refusal must name what the caller can do: {refusal}"
    );
    assert!(refusal.contains("team pair"));
}

#[test]
fn declared_leaf_and_every_non_leaf_are_never_refused() {
    assert!(pair_creation_refusal_for_leaf(true, true).is_none());
    assert!(pair_creation_refusal_for_leaf(false, false).is_none());
    assert!(pair_creation_refusal_for_leaf(false, true).is_none());
}

// WHAT: pair service validation refuses a supplied caller id that is blank or unresolved.
// WHY: A bad caller id must not silently turn a scoped call into a root call.
// ORACLE: the service returns an error naming the unresolved caller tab.
#[test]
fn pair_creation_refuses_unresolvable_supplied_creator_ids() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let error = validate_pair_creator(&mux, Some("missing-caller"), false)
        .expect_err("a supplied but unknown caller must not bypass the hierarchy check");
    assert!(error.contains("missing-caller"), "{error}");
}

// WHAT: Worker and Manager callers need explicit leaf acknowledgement.
// WHY: Valid providers must not let a Team leaf reach pair reservation.
// ORACLE: each call returns the acknowledgement refusal without changing Tabs, Teams, or Plans.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn pair_leaf_call_is_refused_with_registered_providers_before_reservation() {
    use crate::multiplexer::types::AgentRole;

    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let worker_id = mux.create_tab(&workspace_id).unwrap();
    let manager_id = mux.create_tab(&workspace_id).unwrap();
    for (tab_id, role) in [
        (worker_id.as_str(), AgentRole::Worker),
        (manager_id.as_str(), AgentRole::Manager),
    ] {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.role = Some(role);
        tab.agent_type = Some("provider".into());
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("pair-orchestrator"),
    ));
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("pair-planner"),
    ));
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();
    let teams_before = fixture.service.mux.read().team_count();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for creator_tab_id in [&worker_id, &manager_id] {
        let error = runtime
            .block_on(
                fixture
                    .service
                    .create_orchestration_pair(OrchestrationPairParams {
                        orchestrator_agent: "pair-orchestrator".into(),
                        planner_agent: "pair-planner".into(),
                        workspace_id: Some(workspace_id.clone()),
                        spawned_by_tab_id: Some(creator_tab_id.clone()),
                        ..Default::default()
                    }),
            )
            .expect_err("a Team leaf without the explicit acknowledgement must be refused");
        assert!(error.contains("--acknowledge-leaf"), "{error}");
    }

    let mux = fixture.service.mux.read();
    assert_eq!(mux.snapshot().tabs.len(), tabs_before);
    assert_eq!(mux.team_count(), teams_before);
    assert!(fixture.service.plan_store.list_index().is_empty());
}

// WHAT: both pair providers are checked before reserving pair state.
// WHY: an unknown Planner must not strand the otherwise valid Orchestrator Tab.
// ORACLE: no new Tabs, Teams, or Plans remain after the provider refusal.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn team_pair_with_unknown_provider_creates_no_tab() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("valid-orchestrator"),
    ));
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();
    let teams_before = fixture.service.mux.read().team_count();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(
            fixture
                .service
                .create_orchestration_pair(OrchestrationPairParams {
                    orchestrator_agent: "valid-orchestrator".into(),
                    planner_agent: "unknown-planner".into(),
                    workspace_id: Some(workspace_id),
                    ..Default::default()
                }),
        )
        .expect_err("an unknown Planner must be rejected before the Orchestrator Tab is reserved");

    assert!(error.contains("unknown-planner"), "{error}");
    let mux = fixture.service.mux.read();
    assert_eq!(mux.snapshot().tabs.len(), tabs_before);
    assert_eq!(mux.team_count(), teams_before);
    assert!(fixture.service.plan_store.list_index().is_empty());
}

// WHAT: a pair can preflight providers routed to the chatBackend transport.
// WHY: provider readiness for PTY launch commands does not apply to chat backends.
// ORACLE: the registered spawnable backend passes pair preflight without reserving Tabs.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn pair_preflight_accepts_spawnable_chat_backend_providers() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.chatbackend_registry.register(
        "pair-chat-backend",
        std::sync::Arc::new(
            crate::service::spawn_capability::test_backend::SpawnableTestBackend {
                id: "pair-chat-backend",
            },
        ),
    );
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();
    let teams_before = fixture.service.mux.read().team_count();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(
            fixture
                .service
                .create_orchestration_pair(OrchestrationPairParams {
                    orchestrator_agent: "pair-chat-backend".into(),
                    planner_agent: "unknown-planner".into(),
                    workspace_id: Some(workspace_id),
                    ..Default::default()
                }),
        )
        .expect_err("preflight must accept the chat backend before reaching the unknown Planner");

    assert!(error.contains("unknown-planner"), "{error}");
    assert_eq!(
        fixture.service.mux.read().snapshot().tabs.len(),
        tabs_before
    );
    assert_eq!(fixture.service.mux.read().team_count(), teams_before);
}

// WHAT: a failed pair launch removes every Tab and its newly minted Team.
// WHY: either role failing to launch must not leave a half-pair behind.
// ORACLE: rollback returns no errors and both reserved Tab ids and Team id are absent.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn pair_rollback_removes_both_reserved_tabs_and_minted_team() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let reserved = {
        let mut mux = fixture.service.mux.write();
        reserve_pair_tabs(
            &mut mux,
            &workspace_id,
            None,
            false,
            &CorrectiveShape::default(),
        )
        .expect("pair reservation")
    };
    let tab_ids = reserved_member_tab_ids(&reserved);
    let team_id = reserved.team.id.clone();

    let errors = rollback_pair_tabs(&fixture.service, &reserved);

    assert!(errors.is_empty(), "rollback errors: {errors:?}");
    let mux = fixture.service.mux.read();
    assert!(tab_ids.iter().all(|tab_id| mux.get_tab(tab_id).is_none()));
    assert!(mux.get_team(&team_id).is_none());
}

// WHAT: an Orchestrator launch error removes every Tab and Team reserved by the pair.
// WHY: spawn_agent may return before it owns cleanup of the existing pair Tab ids.
// ORACLE: after the failed call, the workspace has no additional Tabs or Teams.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn pair_orchestrator_launch_failure_leaves_no_pair_tabs() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_empty_launch_command(
            "pair-empty-orchestrator",
        ),
    ));
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("pair-valid-planner"),
    ));
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();
    let teams_before = fixture.service.mux.read().team_count();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(
            fixture
                .service
                .create_orchestration_pair(OrchestrationPairParams {
                    orchestrator_agent: "pair-empty-orchestrator".into(),
                    planner_agent: "pair-valid-planner".into(),
                    workspace_id: Some(workspace_id),
                    ..Default::default()
                }),
        )
        .expect_err("the provider passes preflight but supplies no launch command");

    assert!(error.contains("Failed to spawn Orchestrator"), "{error}");
    let mux = fixture.service.mux.read();
    assert_eq!(mux.snapshot().tabs.len(), tabs_before);
    assert_eq!(mux.team_count(), teams_before);
    assert!(fixture.service.plan_store.list_index().is_empty());
}

// WHAT: an Orchestrator launch failure preserves the caller's claimed picker Tab.
// WHY: the pair rollback owns restoring the pre-claim state after spawn returns an error.
// ORACLE: the claimed Tab is unchanged and the pair-created Planner Tab, Team, and Plan are gone.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn pair_orchestrator_launch_failure_preserves_claimed_tab() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let picker_tab_id = mux.create_tab(&workspace_id).unwrap();
    let picker_before = mux.get_tab(&picker_tab_id).unwrap().clone();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_empty_launch_command(
            "pair-empty-orchestrator-claimed",
        ),
    ));
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("pair-valid-planner-claimed"),
    ));
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();
    let teams_before = fixture.service.mux.read().team_count();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(
            fixture
                .service
                .create_orchestration_pair(OrchestrationPairParams {
                    orchestrator_agent: "pair-empty-orchestrator-claimed".into(),
                    planner_agent: "pair-valid-planner-claimed".into(),
                    workspace_id: Some(workspace_id),
                    existing_tab_id: Some(picker_tab_id.clone()),
                    ..Default::default()
                }),
        )
        .expect_err("the Orchestrator provider passes preflight but fails at launch");

    assert!(error.contains("Failed to spawn Orchestrator"), "{error}");
    assert!(
        error.contains("did not resolve a launch command"),
        "{error}"
    );
    let mux = fixture.service.mux.read();
    assert_eq!(mux.snapshot().tabs.len(), tabs_before);
    assert_eq!(mux.team_count(), teams_before);
    let restored = mux
        .get_tab(&picker_tab_id)
        .expect("the caller's claimed picker Tab must remain");
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(&picker_before).unwrap()
    );
    assert!(fixture.service.plan_store.list_index().is_empty());
}

// WHAT: Planner launch failure restores a claimed picker Tab and removes the created Planner Tab.
// WHY: rollback owns the new pair member but must preserve a Tab supplied by the user.
// ORACLE: the original Tab remains blank and no pair-created Tab or Team remains.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn pair_planner_launch_failure_preserves_claimed_tab_and_removes_created_tab() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let picker_tab_id = mux.create_tab(&workspace_id).unwrap();
    let picker_before = mux.get_tab(&picker_tab_id).unwrap().clone();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.chatbackend_registry.register(
        "pair-successful-chat-orchestrator",
        std::sync::Arc::new(
            crate::service::spawn_capability::test_backend::SpawnableTestBackend {
                id: "pair-successful-chat-orchestrator",
            },
        ),
    );
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_empty_launch_command("pair-empty-planner"),
    ));
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();
    let teams_before = fixture.service.mux.read().team_count();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(
            fixture
                .service
                .create_orchestration_pair(OrchestrationPairParams {
                    orchestrator_agent: "pair-successful-chat-orchestrator".into(),
                    planner_agent: "pair-empty-planner".into(),
                    workspace_id: Some(workspace_id),
                    existing_tab_id: Some(picker_tab_id.clone()),
                    ..Default::default()
                }),
        )
        .expect_err("the Planner provider passes preflight but fails at launch");

    assert!(error.contains("Failed to spawn Planner"), "{error}");
    let mux = fixture.service.mux.read();
    assert_eq!(mux.snapshot().tabs.len(), tabs_before);
    assert_eq!(mux.team_count(), teams_before);
    let restored = mux
        .get_tab(&picker_tab_id)
        .expect("the caller's claimed picker Tab must remain");
    assert_eq!(restored.title, picker_before.title);
    assert_eq!(restored.cwd, picker_before.cwd);
    assert_eq!(restored.pty_id, picker_before.pty_id);
    assert_eq!(restored.backing, picker_before.backing);
    assert_eq!(restored.agent_type, picker_before.agent_type);
    assert_eq!(restored.agent_type_locked, picker_before.agent_type_locked);
    assert_eq!(restored.agent_session_id, picker_before.agent_session_id);
    assert_eq!(restored.role, picker_before.role);
    assert_eq!(restored.team_id, picker_before.team_id);
    assert_eq!(restored.agent_booting, picker_before.agent_booting);
    assert_eq!(restored.boot_stage, picker_before.boot_stage);
    assert!(fixture.service.plan_store.list_index().is_empty());
}

#[test]
fn leaf_caller_pair_creation_is_attributed_to_the_leaf_tab_itself() {
    // The declared Manager or Worker must be the recorded creator, never an intermediary shell.
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let leaf_tab_id = mux.create_tab(&workspace_id).unwrap();
    {
        let tab = mux.get_tab_mut(&leaf_tab_id).unwrap();
        tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
        tab.agent_type = Some("provider".into());
    }
    assert!(
        is_pair_creation_leaf(mux.get_tab(&leaf_tab_id).unwrap()),
        "test fixture must actually be a leaf"
    );
    assert!(crate::service::worker_ack::is_agent_spawner(
        mux.get_tab(&leaf_tab_id).unwrap()
    ));
    assert!(
        pair_creation_refusal_for_leaf(true, true).is_none(),
        "a declared leaf must pass the gate"
    );

    let reserved = reserve_pair_tabs(
        &mut mux,
        &workspace_id,
        None,
        false,
        &CorrectiveShape::default(),
    )
    .unwrap();
    let lease = pair_startup_lease(&leaf_tab_id, &reserved).expect("a minted pair Team is leased");
    assert_eq!(
        lease.creator_tab_id, leaf_tab_id,
        "the Team must record the CALLING leaf tab as creator, never an intermediary"
    );
}

#[test]
fn startup_supervision_applies_only_to_agent_spawners() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let caller = mux.create_tab(&workspace_id).unwrap();
    assert!(!crate::service::worker_ack::is_agent_spawner(
        mux.get_tab(&caller).unwrap()
    ));

    let tab = mux.get_tab_mut(&caller).unwrap();
    tab.agent_type = Some("provider".into());
    tab.session_binding_nonce = Some("launch-a".into());
    assert!(crate::service::worker_ack::is_agent_spawner(
        mux.get_tab(&caller).unwrap()
    ));
}

#[test]
fn general_agent_team_guard_refusal_carries_the_requested_pair_shape() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let general = general_agent_in_pair_workspace(&mut mux, &workspace_id);
    let requested_orchestrator = "provider-alpha";
    let requested_planner = "provider-beta";

    let error = reserve_pair_tabs(
        &mut mux,
        &workspace_id,
        Some(&general),
        false,
        &CorrectiveShape::in_workspace(Some(&workspace_id))
            .with_agents(Some(requested_orchestrator), Some(requested_planner)),
    )
    .unwrap_err();

    for supplied in [
        general.as_str(),
        workspace_id.as_str(),
        requested_orchestrator,
        requested_planner,
    ] {
        assert!(
            error.contains(supplied),
            "refusal must name what the caller supplied ({supplied}): {error}"
        );
    }
}

#[test]
fn pair_params_deserialize_socket_caller_and_claimed_tab() {
    // Socket and Tauri paths carry this context in params; REST stamps it from the caller header.
    let body = serde_json::json!({
        "orchestrator_agent": "provider-alpha",
        "planner_agent": "provider-beta",
        "workspace_id": "ws_target_7",
        "existing_tab_id": "tab_general",
        "spawned_by_tab_id": "tab_caller",
    });
    let params: OrchestrationPairParams = serde_json::from_value(body).unwrap();
    assert_eq!(params.existing_tab_id.as_deref(), Some("tab_general"));
    assert_eq!(params.spawned_by_tab_id.as_deref(), Some("tab_caller"));
    assert_eq!(params.workspace_id.as_deref(), Some("ws_target_7"));
}

#[test]
fn leaf_creation_acknowledged_defaults_false_for_every_existing_transport() {
    // REST, socket and the Tauri command all deserialize this one struct, and none of them send the new field yet — a body without it must still gate a leaf caller, never silently opt it in.
    let body = serde_json::json!({
        "orchestrator_agent": "provider-alpha",
        "planner_agent": "provider-beta",
    });
    let params: OrchestrationPairParams = serde_json::from_value(body).unwrap();
    assert!(!params.leaf_creation_acknowledged);
}

#[test]
fn pair_params_carry_reasoning_effort_for_both_roles() {
    let body = serde_json::json!({
        "orchestrator_agent": "opencode",
        "orchestrator_reasoning_effort": "high",
        "planner_agent": "opencode",
        "planner_reasoning_effort": "low",
    });
    let params: OrchestrationPairParams = serde_json::from_value(body).unwrap();

    assert_eq!(
        params.orchestrator_reasoning_effort.as_deref(),
        Some("high")
    );
    assert_eq!(params.planner_reasoning_effort.as_deref(), Some("low"));
}
