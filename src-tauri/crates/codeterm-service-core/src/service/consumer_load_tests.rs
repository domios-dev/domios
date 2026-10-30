//! Service consumers that used to scan every plan now ask the store for the plans
//! they match; each test proves a non-matching plan is never loaded.

use crate::multiplexer::state::MultiplexerState;
use crate::plan::{PlanStatus, TaskStatus};
use crate::service::plan::load_audit::{
    assert_never_loaded, attempt, execution_plan, planner, seed, start, task,
};
use crate::service::test_fixture::ServiceFixture;
use crate::service::worker_interruption::WorkerInterruptionEvent;
use std::collections::HashSet;

fn fixture() -> (ServiceFixture, String) {
    let (mux, orchestrator) = MultiplexerState::new();
    (ServiceFixture::new(mux), orchestrator)
}

fn project(fx: &ServiceFixture) -> std::path::PathBuf {
    fx.temp.path().to_path_buf()
}

#[test]
fn system_entry_views_for_tab_never_loads_unmatched_plans() {
    let (fx, orchestrator) = fixture();
    let store = &fx.service.plan_store;
    let owned = seed(
        store,
        execution_plan(
            &project(&fx),
            "owned",
            &orchestrator,
            vec![task("task_o", TaskStatus::InProgress, None)],
        ),
    );
    let other = seed(
        store,
        execution_plan(
            &project(&fx),
            "other",
            "other-owner",
            vec![task("task_x", TaskStatus::InProgress, None)],
        ),
    );
    start(store);

    let views = crate::delayed_send::system_heartbeat_views(&fx.service, &orchestrator).unwrap();
    assert!(views.len() <= 1);
    assert_eq!(store.plan_loads_for_test(), vec![owned]);
    assert_never_loaded(store, &[&other]);
}

#[test]
fn in_progress_assignments_for_tab_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mine = seed(
        store,
        execution_plan(
            &project(&fx),
            "mine",
            "owner",
            vec![task("task_m", TaskStatus::InProgress, Some("worker-a"))],
        ),
    );
    let other = seed(
        store,
        execution_plan(
            &project(&fx),
            "other",
            "owner",
            vec![task("task_x", TaskStatus::InProgress, Some("worker-b"))],
        ),
    );
    start(store);

    let losses = fx.service.in_progress_assignments_for_tab("worker-a");
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].plan_id, mine);
    assert_never_loaded(store, &[&other]);
}

#[test]
fn worker_losses_for_tab_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    seed(
        store,
        execution_plan(
            &project(&fx),
            "mine",
            "owner",
            vec![task("task_m", TaskStatus::InProgress, Some("worker-a"))],
        ),
    );
    let pending = seed(
        store,
        execution_plan(
            &project(&fx),
            "pending",
            "owner",
            vec![task("task_p", TaskStatus::Pending, Some("worker-a"))],
        ),
    );
    start(store);

    let losses = fx.service.worker_losses_for_tab(
        "worker-a",
        crate::service::worker_loss::WorkerLossCause::AgentUnavailable,
    );
    assert_eq!(losses.len(), 1);
    assert_never_loaded(store, &[&pending]);
}

#[test]
fn worker_is_task_bound_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut held = task("task_m", TaskStatus::Done, None);
    held.execution_attempts.push(attempt("att_m", "worker-a"));
    seed(
        store,
        execution_plan(&project(&fx), "held", "owner", vec![held]),
    );
    let other = seed(
        store,
        execution_plan(
            &project(&fx),
            "other",
            "owner",
            vec![task("task_x", TaskStatus::InProgress, Some("worker-b"))],
        ),
    );
    start(store);

    assert!(crate::service::agent_transition::worker_is_task_bound(
        store, "worker-a"
    ));
    assert!(!crate::service::agent_transition::worker_is_task_bound(
        store, "worker-c"
    ));
    assert_never_loaded(store, &[&other]);
}

#[test]
fn worker_task_bindings_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mine = seed(
        store,
        execution_plan(
            &project(&fx),
            "mine",
            "owner",
            vec![task("task_m", TaskStatus::InProgress, Some("worker-a"))],
        ),
    );
    let finished = seed(
        store,
        execution_plan(
            &project(&fx),
            "finished",
            "owner",
            vec![task("task_f", TaskStatus::Done, Some("worker-a"))],
        ),
    );
    start(store);

    assert_eq!(
        crate::service::agent_transition::worker_task_bindings(store, "worker-a"),
        vec![(mine, "task_m".to_string())]
    );
    assert_never_loaded(store, &[&finished]);
}

#[test]
fn restore_lane_watches_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut laned = task("task_l", TaskStatus::InProgress, Some("worker-a"));
    laned.lane.worktree_path = Some(fx.temp.path().join("lane").display().to_string());
    let laned = seed(
        store,
        execution_plan(&project(&fx), "laned", "owner", vec![laned]),
    );
    let mut settled = task("task_s", TaskStatus::Done, None);
    settled.lane.worktree_path = Some(fx.temp.path().join("old-lane").display().to_string());
    let settled = seed(
        store,
        execution_plan(&project(&fx), "settled", "owner", vec![settled]),
    );
    let laneless = seed(
        store,
        execution_plan(
            &project(&fx),
            "laneless",
            "owner",
            vec![task("task_n", TaskStatus::InProgress, None)],
        ),
    );
    start(store);

    fx.service.restore_lane_watches();
    assert_eq!(store.plan_loads_for_test(), vec![laned]);
    assert_never_loaded(store, &[&settled, &laneless]);
}

#[test]
fn find_lane_attempt_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut laned = task("task_l", TaskStatus::InProgress, Some("worker-a"));
    let mut lane_attempt = attempt("att_l", "worker-a");
    lane_attempt.worktree_path = Some(fx.temp.path().join("lane").display().to_string());
    laned.execution_attempts.push(lane_attempt);
    seed(
        store,
        execution_plan(&project(&fx), "laned", "owner", vec![laned]),
    );
    let mut other = task("task_x", TaskStatus::InProgress, Some("worker-b"));
    other.execution_attempts.push(attempt("att_x", "worker-b"));
    let other = seed(
        store,
        execution_plan(&project(&fx), "other", "owner", vec![other]),
    );
    start(store);

    assert!(crate::service::lane_stats::find_lane_attempt(store, "worker-a").is_some());
    assert_never_loaded(store, &[&other]);
}

#[test]
fn settle_lanes_held_by_closed_tab_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut held = task("task_h", TaskStatus::Done, Some("closed-tab"));
    held.lane.branch = Some("lane/held".into());
    let held = seed(
        store,
        execution_plan(&project(&fx), "held", "owner", vec![held]),
    );
    let running = seed(
        store,
        execution_plan(
            &project(&fx),
            "running",
            "owner",
            vec![task("task_r", TaskStatus::InProgress, Some("closed-tab"))],
        ),
    );
    let mut elsewhere = task("task_e", TaskStatus::Done, Some("live-tab"));
    elsewhere.lane.branch = Some("lane/else".into());
    let elsewhere = seed(
        store,
        execution_plan(&project(&fx), "elsewhere", "owner", vec![elsewhere]),
    );
    start(store);

    fx.service.settle_lanes_held_by_closed_tab("closed-tab");
    assert!(store.plan_loads_for_test().contains(&held));
    assert_never_loaded(store, &[&running, &elsewhere]);
}

#[test]
fn bind_planner_session_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let planner_tab = {
        let mut mux = fx.service.mux.write();
        let workspace = mux.list_workspaces()[0].id.clone();
        let tab_id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("planner-session".into());
        tab.session_binding_nonce = Some("planner-nonce".into());
        tab.reported_session_binding_nonce = Some("planner-nonce".into());
        tab_id
    };
    let store = &fx.service.plan_store;
    let mut planned = execution_plan(&project(&fx), "planned", "owner", vec![]);
    planned.planner = planner(&planner_tab);
    let planned = seed(store, planned);
    let mut other = execution_plan(&project(&fx), "other", "owner", vec![]);
    other.planner = planner("other-planner");
    let other = seed(store, other);
    start(store);

    fx.service.bind_planner_session(&planner_tab);
    assert!(store.plan_loads_for_test().contains(&planned));
    assert_never_loaded(store, &[&other]);
}

#[test]
fn signal_provider_exhaustion_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut exhausted = task("task_c", TaskStatus::InProgress, Some("worker-a"));
    let mut claude = attempt("att_c", "worker-a");
    claude.provider = Some("claude".into());
    exhausted.execution_attempts.push(claude);
    let exhausted = seed(
        store,
        execution_plan(&project(&fx), "claude", "owner", vec![exhausted]),
    );
    let mut fine = task("task_x", TaskStatus::InProgress, Some("worker-b"));
    let mut codex = attempt("att_x", "worker-b");
    codex.provider = Some("codex".into());
    fine.execution_attempts.push(codex);
    let fine = seed(
        store,
        execution_plan(&project(&fx), "codex", "owner", vec![fine]),
    );
    start(store);

    assert_eq!(
        fx.service.signal_provider_exhaustion("claude"),
        0,
        "no task names an interruption owner"
    );
    assert!(store.plan_loads_for_test().contains(&exhausted));
    assert_never_loaded(store, &[&fine]);
}

#[test]
fn signal_provider_recovery_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut paused = task("task_p", TaskStatus::Paused, None);
    paused.status_label = Some(crate::service::plan::interruption::PROVIDER_EXHAUSTED_LABEL.into());
    paused.interruption.owner_tab_id = Some("worker-a".into());
    let mut codex = attempt("att_p", "worker-a");
    codex.provider = Some("codex".into());
    paused.execution_attempts.push(codex);
    let paused = seed(
        store,
        execution_plan(&project(&fx), "paused", "owner", vec![paused]),
    );
    let running = seed(
        store,
        execution_plan(
            &project(&fx),
            "running",
            "owner",
            vec![task("task_r", TaskStatus::InProgress, Some("worker-b"))],
        ),
    );
    start(store);

    assert_eq!(
        fx.service.signal_provider_recovery("claude"),
        0,
        "another provider paused it"
    );
    assert_eq!(store.plan_loads_for_test(), vec![paused]);
    assert_never_loaded(store, &[&running]);
}

#[test]
fn signal_worker_interruption_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut owned = task("task_o", TaskStatus::InProgress, Some("worker-a"));
    owned.interruption.owner_tab_id = Some("worker-a".into());
    owned.interruption.execution_intent = crate::plan::ExecutionIntent::Mutating;
    owned.execution_attempts.push(attempt("att_o", "worker-a"));
    let owned = seed(
        store,
        execution_plan(&project(&fx), "owned", "owner", vec![owned]),
    );
    let mut other = task("task_x", TaskStatus::InProgress, Some("worker-b"));
    other.interruption.owner_tab_id = Some("worker-b".into());
    other.interruption.execution_intent = crate::plan::ExecutionIntent::Mutating;
    other.execution_attempts.push(attempt("att_x", "worker-b"));
    let other = seed(
        store,
        execution_plan(&project(&fx), "other", "owner", vec![other]),
    );
    start(store);

    assert!(
        !fx.service
            .signal_worker_interruption("worker-a", WorkerInterruptionEvent::ProviderExhausted),
        "an owner without a session cannot be checkpointed"
    );
    assert_eq!(store.plan_loads_for_test(), vec![owned]);
    assert_never_loaded(store, &[&other]);
}

#[test]
fn clear_planner_bindings_for_lifecycle_exit_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut planned = execution_plan(&project(&fx), "planned", "owner", vec![]);
    planned.planner = planner("planner-a");
    seed(store, planned);
    let mut other = execution_plan(&project(&fx), "other", "owner", vec![]);
    other.planner = planner("planner-b");
    let other = seed(store, other);
    start(store);

    assert_eq!(
        crate::service::hierarchy::clear_planner_bindings_for_lifecycle_exit(store, "planner-a", 5),
        1
    );
    assert_never_loaded(store, &[&other]);
}

#[test]
fn lazy_restore_initialize_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut continuing = task("task_c", TaskStatus::InProgress, Some("worker-a"));
    continuing
        .execution_attempts
        .push(attempt("att_c", "worker-a"));
    let continuing = seed(
        store,
        execution_plan(&project(&fx), "continuing", "owner", vec![continuing]),
    );
    let idle = seed(
        store,
        execution_plan(
            &project(&fx),
            "idle",
            "owner",
            vec![task("task_i", TaskStatus::Pending, None)],
        ),
    );
    start(store);

    crate::service::lazy_restore::initialize(&fx.service, 24);
    assert_eq!(store.plan_loads_for_test(), vec![continuing]);
    assert_never_loaded(store, &[&idle]);
}

#[test]
fn run_lane_watchdog_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let running = seed(
        store,
        execution_plan(
            &project(&fx),
            "running",
            "owner",
            vec![task("task_r", TaskStatus::InProgress, Some("worker-a"))],
        ),
    );
    let queued = seed(
        store,
        execution_plan(
            &project(&fx),
            "queued",
            "owner",
            vec![task("task_q", TaskStatus::Pending, None)],
        ),
    );
    let mut archived = execution_plan(
        &project(&fx),
        "archived",
        "owner",
        vec![task("task_a", TaskStatus::InProgress, Some("worker-b"))],
    );
    archived.status = PlanStatus::Archived;
    let archived = seed(store, archived);
    start(store);

    fx.service.run_lane_watchdog();
    assert!(store.plan_loads_for_test().contains(&running));
    assert_never_loaded(store, &[&queued, &archived]);
}

#[test]
fn curator_bootstrap_ids_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let active = seed(
        store,
        execution_plan(&project(&fx), "active", "owner", vec![]),
    );
    let mut scheduled = execution_plan(&project(&fx), "scheduled", "owner", vec![]);
    scheduled.status = PlanStatus::Archived;
    scheduled.control_loop.next_at = Some(10);
    let scheduled = seed(store, scheduled);
    let mut quiet = execution_plan(&project(&fx), "quiet", "owner", vec![]);
    quiet.status = PlanStatus::Archived;
    let quiet = seed(store, quiet);
    start(store);

    let ids = crate::service::curator::curator_bootstrap_ids(store);
    assert_eq!(ids, [active, scheduled].into_iter().collect());
    assert!(!ids.contains(&quiet));
    assert!(
        store.plan_loads_for_test().is_empty(),
        "bootstrap names plans from SQL"
    );
}

#[test]
fn reconcile_notification_questions_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let clock = crate::cron::clock::MockClock::starting_at(1_700_000_000_000);
    let cron = std::sync::Arc::new(parking_lot::Mutex::new(
        crate::cron::store::CronStore::new_with_path_and_clock(
            fx.temp.path().join("crons.json"),
            clock,
        ),
    ));
    assert!(fx.service.set_ext(cron).is_ok());
    let store = &fx.service.plan_store;
    let active = seed(
        store,
        execution_plan(&project(&fx), "active", "owner", vec![]),
    );
    let mut scheduled = execution_plan(&project(&fx), "scheduled", "owner", vec![]);
    scheduled.status = PlanStatus::Archived;
    scheduled.control_loop.next_at = Some(10);
    let scheduled = seed(store, scheduled);
    let mut quiet = execution_plan(&project(&fx), "quiet", "owner", vec![]);
    quiet.status = PlanStatus::Archived;
    let quiet = seed(store, quiet);
    start(store);

    fx.service.reconcile_notification_questions();

    let loaded = store.plan_loads_for_test();
    assert!(loaded.contains(&active));
    assert!(loaded.contains(&scheduled));
    assert_never_loaded(store, &[&quiet]);
}

#[test]
fn is_self_close_protected_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    seed(
        store,
        execution_plan(&project(&fx), "owned", "owner-tab", vec![]),
    );
    let other = seed(
        store,
        execution_plan(&project(&fx), "other", "someone-else", vec![]),
    );
    start(store);

    assert!(fx.service.is_self_close_protected("owner-tab"));
    assert_never_loaded(store, &[&other]);
}

#[test]
fn bound_plan_id_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut team_plan = execution_plan(&project(&fx), "team", "owner", vec![]);
    team_plan.team_id = Some("team-a".into());
    let team_plan = seed(store, team_plan);
    let other = seed(
        store,
        execution_plan(&project(&fx), "other", "someone-else", vec![]),
    );
    start(store);

    assert_eq!(
        crate::service::orchestrator::bound_plan_id(store, "member-tab", Some("team-a")),
        Some(team_plan)
    );
    assert_never_loaded(store, &[&other]);
}

#[test]
fn close_inventory_plans_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut closing = execution_plan(&project(&fx), "closing", "owner", vec![]);
    closing.team_id = Some("team-closing".into());
    let closing = seed(store, closing);
    let mut staying = execution_plan(&project(&fx), "staying", "owner", vec![]);
    staying.team_id = Some("team-staying".into());
    let staying = seed(store, staying);
    start(store);

    let teams: HashSet<&str> = ["team-closing"].into_iter().collect();
    let plans = crate::service::close_history::close_inventory_plans(store, &teams);
    assert_eq!(
        plans.into_iter().map(|plan| plan.id).collect::<Vec<_>>(),
        vec![closing]
    );
    assert_never_loaded(store, &[&staying]);
}
