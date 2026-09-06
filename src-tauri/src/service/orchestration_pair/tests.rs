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
fn degraded_path_creates_no_plan_when_planner_spawn_fails() {
    let (_root, store) = store_for_test();
    assert!(
        store.list_index().is_empty(),
        "degraded pair spawn must not leave an unowned empty plan"
    );
}

#[test]
fn degraded_fix_command_includes_model_when_present() {
    let cmd = degraded_fix_command("orch-1", "codex", Some("gpt-5"));
    assert!(cmd.contains("agent spawn codex"));
    assert!(cmd.contains("--model gpt-5"));
    assert!(cmd.contains("--parent-tab orch-1"));
    assert!(cmd.contains("--role Planner"));
    assert!(!cmd.contains("--plan"));
}

#[test]
fn degraded_fix_command_omits_model_flag_when_absent() {
    let cmd = degraded_fix_command("orch-1", "codex", None);
    assert!(!cmd.contains("--model"));
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
fn pair_followup_refuses_bare_shell_and_booting_agent() {
    use codeterm_types::pty::BootStage;

    assert!(!pair_agent_state_allows_followup(
        false,
        Some(BootStage::SpawnFailed),
        false,
        false
    ));
    assert!(!pair_agent_state_allows_followup(
        false,
        Some(BootStage::AgentReady),
        false,
        false
    ));
    assert!(!pair_agent_state_allows_followup(
        true,
        Some(BootStage::AgentReady),
        false,
        true
    ));
    assert!(pair_agent_state_allows_followup(
        false,
        Some(BootStage::AgentReady),
        false,
        true
    ));
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
    let reserved =
        reserve_pair_tabs(&mut mux, &workspace_id, None, &CorrectiveShape::default()).unwrap();

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
    let reserved =
        reserve_pair_tabs(&mut mux, &workspace_id, None, &CorrectiveShape::default()).unwrap();
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

#[test]
fn degraded_followup_names_exact_fix_command_and_waiver_hint() {
    let fix = degraded_fix_command("orch-1", "codex", None);
    let msg = orchestrator_pair_followup_degraded(&fix, "spawn timed out");
    assert!(msg.contains("spawn timed out"));
    assert!(msg.contains(&fix));
    assert!(msg.contains(crate::prompts::PEER_WAIVER_HINT));
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

#[test]
fn leaf_caller_pair_creation_is_attributed_to_the_leaf_tab_itself() {
    // A Manager/Worker tab that declares its intent must be the recorded
    // creator — never an intermediary throwaway shell (the workaround #400
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

    let reserved =
        reserve_pair_tabs(&mut mux, &workspace_id, None, &CorrectiveShape::default()).unwrap();
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
fn general_agent_team_guard_transports_forward_the_claimed_tab_to_the_guarded_service() {
    // REST, socket and the Tauri command all deserialize this one struct
    // before calling `create_orchestration_pair`, so none of them can drop
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
    // REST, socket and the Tauri command all deserialize this one struct, and
    // none of them send the new field yet — a body without it must still gate
    let body = serde_json::json!({
        "orchestrator_agent": "provider-alpha",
        "planner_agent": "provider-beta",
    });
    let params: OrchestrationPairParams = serde_json::from_value(body).unwrap();
    assert!(!params.leaf_creation_acknowledged);
}
