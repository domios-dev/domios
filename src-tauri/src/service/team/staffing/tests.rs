//! codeterm#418: a Team whose Orchestrator is gone can receive another, and
//! the replacement takes the plan over through the tab-gone recovery rebind.

use super::*;

use crate::api::events::EventBroadcaster;
use crate::plan::{OrchestratorRef, Plan, PlannerRef};
use crate::service::hierarchy::plan_reminder_nudge::{
    timed_plan_reminder_targets, PlanReminderTarget,
};
use crate::service::plan::PlanStore;

/// An Orchestrator-anchored Team with a Planner and a Worker, where the anchor
/// carries an agent identity — the shape a live Team actually has.
fn live_team() -> (MultiplexerState, String, String, String, String) {
    let (mut mux, orchestrator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    give_agent_identity(&mut mux, &orchestrator);
    let planner = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(
        &planner,
        AgentRole::Manager,
        Some("planner"),
        Some(&orchestrator),
    )
    .unwrap();
    give_agent_identity(&mut mux, &planner);
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orchestrator))
        .unwrap();
    give_agent_identity(&mut mux, &worker);
    (mux, team_id, orchestrator, planner, worker)
}

fn give_agent_identity(mux: &mut MultiplexerState, tab_id: &str) {
    let tab = mux.get_tab_mut(tab_id).expect("fixture tab exists");
    tab.agent_type = Some("provider-neutral".into());
}

fn orchestrator_params() -> AddTeamMemberParams {
    AddTeamMemberParams {
        provider: "spawn-capable".into(),
        role: "orchestrator".into(),
        title: None,
        task: None,
        model: None,
    }
}

fn cartographer_params() -> AddTeamMemberParams {
    AddTeamMemberParams {
        provider: "provider-neutral".into(),
        role: "cartographer".into(),
        title: None,
        task: None,
        model: None,
    }
}

#[test]
fn cartographer_staffing_uses_the_manager_role_kind_and_team_parent() {
    let (mux, team_id, orchestrator, _, _) = live_team();
    let prepared =
        prepare_team_member_spawn(&mux, &team_id, Some(&orchestrator), cartographer_params())
            .expect("a spawn-capable Team manager can attach a Cartographer");

    assert_eq!(prepared.expected_role, "Manager");
    assert_eq!(prepared.expected_profile.as_deref(), Some(CARTOGRAPHER));
    assert_eq!(
        prepared.spawn_params.parent_tab_id.as_deref(),
        Some(orchestrator.as_str())
    );
    assert!(!role_kind::anchors_team(
        Some(AgentRole::Manager),
        prepared.expected_profile.as_deref()
    ));
}

#[test]
fn an_existing_cartographer_attachment_is_reused_as_a_singleton() {
    let (mut mux, team_id, orchestrator, _, _) = live_team();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let cartographer = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(
        &cartographer,
        AgentRole::Manager,
        Some(CARTOGRAPHER),
        Some(&orchestrator),
    )
    .unwrap();
    give_agent_identity(&mut mux, &cartographer);

    let reused = existing_cartographer_attachment(&mux, &team_id)
        .expect("the attached Manager is the singleton result");
    assert_eq!(reused.tab_id, cartographer);
    assert_eq!(reused.team.id, team_id);
    assert_eq!(reused.role_type, "Manager");
    assert_eq!(reused.role_profile.as_deref(), Some(CARTOGRAPHER));
    assert!(reused.reused);
    assert_eq!(
        mux.snapshot()
            .tabs
            .values()
            .filter(|tab| {
                tab.team_id.as_deref() == Some(team_id.as_str())
                    && tab.role_kind_id() == Some(CARTOGRAPHER)
            })
            .count(),
        1
    );
}

#[test]
fn cartographer_attachment_lock_is_shared_per_team_only() {
    let locks = TeamCartographerAttachLocks::default();
    let first = locks.for_team("team_a");
    let repeated = locks.for_team("team_a");
    let other = locks.for_team("team_b");

    assert!(Arc::ptr_eq(&first, &repeated));
    assert!(!Arc::ptr_eq(&first, &other));
}

#[cfg(windows)]
#[tokio::test]
async fn repeated_service_cartographer_attachment_returns_the_existing_tab() {
    let (mut mux, team_id, orchestrator, _, _) = live_team();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let cartographer = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(
        &cartographer,
        AgentRole::Manager,
        Some(CARTOGRAPHER),
        Some(&orchestrator),
    )
    .unwrap();
    give_agent_identity(&mut mux, &cartographer);
    let initial_tab_count = mux.snapshot().tabs.len();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    let (first, repeated) = tokio::join!(
        fixture
            .service
            .add_team_member(&team_id, Some(&orchestrator), cartographer_params(),),
        fixture
            .service
            .add_team_member(&team_id, Some(&orchestrator), cartographer_params(),),
    );
    let first = first.expect("first attachment reuses the existing role instance");
    let repeated = repeated.expect("repeated attachment reuses the same role instance");

    assert_eq!(first.tab_id, cartographer);
    assert_eq!(first.tab_id, repeated.tab_id);
    assert_eq!(first.role_type, "Manager");
    assert_eq!(first.role_profile.as_deref(), Some(CARTOGRAPHER));
    assert!(first.reused && repeated.reused);
    let mux = fixture.service.mux.read();
    assert_eq!(mux.snapshot().tabs.len(), initial_tab_count);
    assert_eq!(
        mux.snapshot()
            .tabs
            .values()
            .filter(|tab| {
                tab.team_id.as_deref() == Some(team_id.as_str())
                    && tab.role_kind_id() == Some(CARTOGRAPHER)
            })
            .count(),
        1
    );
}

/// Place a replacement anchor the way a real spawn would, then commit it.
fn commit_replacement_anchor(
    mux: &mut MultiplexerState,
    team_id: &str,
    prepared: &PreparedTeamMemberSpawn,
) -> AddTeamMemberResult {
    let workspace_id = mux
        .get_team(team_id)
        .map(|team| team.workspace_id.clone())
        .expect("team exists");
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&tab_id).unwrap();
    give_agent_identity(mux, &tab_id);
    commit_team_member_spawn(
        mux,
        &EventBroadcaster::new(),
        team_id,
        None,
        &tab_id,
        &prepared.expected_role,
        &prepared.expected_profile,
    )
    .expect("a headless Team accepts the replacement anchor")
}

/// An executing Team plan with open work — the shape the heartbeat targets.
fn team_plan(store: &PlanStore, team_id: &str, owner: &str, planner: &str, cwd: &str) -> String {
    let mut plan = Plan::new(
        "Forge Architecture Audit".into(),
        cwd.into(),
        OrchestratorRef::from_tab(owner),
    );
    plan.team_id = Some(team_id.to_string());
    plan.mode = crate::plan::PlanMode::Execution;
    plan.planner = Some(PlannerRef::Tab {
        tab_id: planner.to_string(),
        session_id: None,
        title: None,
    });
    store.save(&mut plan, 1).unwrap();
    store
        .add_task(
            &plan.id,
            crate::service::plan::NewTask {
                name: "Audit the forge".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    plan.id
}

#[test]
fn live_orchestrator_count_ignores_a_removed_or_dead_anchor() {
    let (mut mux, team_id, orchestrator, _planner, _worker) = live_team();
    assert_eq!(
        live_team_orchestrators(&mux, &team_id),
        vec![orchestrator.clone()],
        "an anchor holding an agent identity counts"
    );

    // Lifecycle exit clears the identity and leaves the tab behind.
    mux.get_tab_mut(&orchestrator).unwrap().agent_type = None;
    assert!(
        live_team_orchestrators(&mux, &team_id).is_empty(),
        "a dead Orchestrator is a record, not a live anchor"
    );

    give_agent_identity(&mut mux, &orchestrator);
    mux.close_tab(&orchestrator).unwrap();
    assert!(
        live_team_orchestrators(&mux, &team_id).is_empty(),
        "a removed Orchestrator does not count"
    );
}

#[test]
fn a_headless_team_accepts_a_new_orchestrator_with_no_caller() {
    let (mut mux, team_id, orchestrator, planner, _worker) = live_team();
    mux.close_tab(&orchestrator).unwrap();
    assert!(
        mux.get_tab(&planner).is_some(),
        "members outlive the anchor"
    );

    let prepared = prepare_team_member_spawn(&mux, &team_id, None, orchestrator_params())
        .expect("a Team with no live Orchestrator accepts one");
    assert_eq!(prepared.expected_role, "Manager");
    assert_eq!(prepared.expected_profile.as_deref(), Some("orchestrator"));
    assert_eq!(prepared.spawn_params.parent_tab_id, None);

    let result = commit_replacement_anchor(&mut mux, &team_id, &prepared);
    assert_eq!(result.team.id, team_id);
    assert_eq!(
        live_team_orchestrators(&mux, &team_id),
        vec![result.tab_id.clone()],
        "the Team is anchored again"
    );
}

#[test]
fn a_headless_team_accepts_a_new_orchestrator_from_a_spawn_capable_member() {
    let (mut mux, team_id, orchestrator, planner, _worker) = live_team();
    mux.close_tab(&orchestrator).unwrap();
    mux.set_can_spawn(&planner, true).unwrap();

    prepare_team_member_spawn(&mux, &team_id, Some(&planner), orchestrator_params())
        .expect("the UI add-member action lands on the same staffing path");
}

#[test]
fn a_foreign_team_agent_cannot_replace_the_missing_orchestrator() {
    let (mut mux, team_id, orchestrator, _planner, _worker) = live_team();
    mux.close_tab(&orchestrator).unwrap();
    let workspace_id = mux.get_team(&team_id).unwrap().workspace_id.clone();
    let foreign = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&foreign).unwrap();
    give_agent_identity(&mut mux, &foreign);
    mux.resolve_or_mint_team_for_parent(&foreign).unwrap();

    let error = prepare_team_member_spawn(&mux, &team_id, Some(&foreign), orchestrator_params())
        .unwrap_err();

    assert!(error.starts_with(codes::TEAM_CALLER_UNAUTHORIZED));
    assert!(live_team_orchestrators(&mux, &team_id).is_empty());
}

#[test]
fn a_second_live_orchestrator_is_refused_by_error_code_constant() {
    let (mux, team_id, orchestrator, _planner, _worker) = live_team();

    for caller in [None, Some(orchestrator.as_str())] {
        let error = prepare_team_member_spawn(&mux, &team_id, caller, orchestrator_params())
            .expect_err("a Team holds at most one live Orchestrator");
        assert!(
            error.starts_with(codes::TEAM_ORCHESTRATOR_ALREADY_LIVE),
            "the refusal must lead with its code, got: {error}"
        );
        assert!(
            error.contains(&orchestrator),
            "the refusal must name the incumbent, got: {error}"
        );
    }
}

#[test]
fn a_refused_unknown_role_names_its_own_code() {
    let (mux, team_id, orchestrator, _planner, _worker) = live_team();
    let error = prepare_team_member_spawn(
        &mux,
        &team_id,
        Some(&orchestrator),
        AddTeamMemberParams {
            role: "overseer".into(),
            ..orchestrator_params()
        },
    )
    .expect_err("only the three staffing roles are placeable");
    assert!(
        error.starts_with(codes::TEAM_MEMBER_ROLE_UNKNOWN),
        "got: {error}"
    );
}

#[test]
fn the_new_orchestrator_takes_the_plan_over_and_the_heartbeat_follows() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let (mut mux, team_id, orchestrator, planner, _worker) = live_team();
    let plan_id = team_plan(
        &store,
        &team_id,
        &orchestrator,
        &planner,
        &dir.path().display().to_string(),
    );
    mux.close_tab(&orchestrator).unwrap();

    let prepared = prepare_team_member_spawn(&mux, &team_id, None, orchestrator_params()).unwrap();
    let result = commit_replacement_anchor(&mut mux, &team_id, &prepared);
    let takeover = take_over_team_plan(&mut mux, &store, &team_id, &result.tab_id, 7)
        .expect("a plan whose owner tab is gone is taken over");
    assert_eq!(takeover.error, None, "the recovery rebind must succeed");
    assert_eq!(takeover.plan_id, plan_id);

    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.owner.tab_id.as_deref(), Some(result.tab_id.as_str()));
    assert_eq!(plan.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(plan.status, crate::plan::PlanStatus::Active);
    assert_eq!(
        plan.planner,
        Some(PlannerRef::Tab {
            tab_id: planner.clone(),
            session_id: None,
            title: None,
        }),
        "the Team's own Planner survives the recovery"
    );
    let transfer = plan.owner_history.last().expect("ownership was recorded");
    assert_eq!(transfer.from.tab_id.as_deref(), Some(orchestrator.as_str()));
    assert_eq!(transfer.to.tab_id.as_deref(), Some(result.tab_id.as_str()));

    let reloaded = store.get(&plan_id).unwrap();
    let plans = [reloaded];
    let reminder_targets = timed_plan_reminder_targets(&plans);
    let owner_targets: Vec<&str> = reminder_targets
        .iter()
        .filter(|target| matches!(&target.target, PlanReminderTarget::Owner { .. }))
        .map(|target| target.tab_id.as_str())
        .collect();
    assert_eq!(
        owner_targets,
        vec![result.tab_id.as_str()],
        "the plan heartbeat targets the new Orchestrator"
    );
}

#[test]
fn a_plan_whose_owner_is_still_a_live_anchor_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let (mut mux, team_id, orchestrator, planner, _worker) = live_team();
    let plan_id = team_plan(
        &store,
        &team_id,
        &orchestrator,
        &planner,
        &dir.path().display().to_string(),
    );

    assert!(
        take_over_team_plan(&mut mux, &store, &team_id, &orchestrator, 7).is_none(),
        "the recorded owner is the live anchor already"
    );
    assert_eq!(
        store.get(&plan_id).unwrap().owner.tab_id.as_deref(),
        Some(orchestrator.as_str())
    );
}
