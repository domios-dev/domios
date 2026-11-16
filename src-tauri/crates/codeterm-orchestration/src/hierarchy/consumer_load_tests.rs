use crate::test_fixture::ServiceFixture;
use crate::traits::HierarchyPlannerDeliveryExt;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::plan::{PlanStatus, TaskStatus};
use codeterm_service_core::service::plan::load_audit::{
    assert_never_loaded, execution_plan, planner, seed, start, task,
};
fn fixture() -> (ServiceFixture, String) {
    let (mux, id) = MultiplexerState::new();
    (ServiceFixture::new(mux), id)
}
fn project(fx: &ServiceFixture) -> std::path::PathBuf {
    fx.temp.path().to_path_buf()
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

    fx.service.bind_planner_session(&planner_tab).unwrap();
    let selected = store.planner_gate_context(&planned, None).unwrap().unwrap();
    assert!(matches!(selected.planner,
        Some(codeterm_service_core::plan::PlannerRef::Tab { tab_id, session_id: Some(session), .. })
            if tab_id == planner_tab && session == "planner-session"));
    let unmatched = store.planner_gate_context(&other, None).unwrap().unwrap();
    assert!(matches!(unmatched.planner,
        Some(codeterm_service_core::plan::PlannerRef::Tab { tab_id, session_id: Some(session), .. })
            if tab_id == "other-planner" && session == "other-planner-session"));
    assert!(store.plan_loads_for_test().is_empty());
    assert_never_loaded(store, &[&other]);
}

#[test]
fn system_entry_views_for_tab_never_loads_unmatched_plans() {
    let (fx, orchestrator) = fixture();
    let store = &fx.service.plan_store;
    let mut owned_plan = execution_plan(
        &project(&fx),
        "owned",
        &orchestrator,
        vec![task("task_o", TaskStatus::InProgress, None)],
    );
    owned_plan.control_loop.mode = codeterm_service_core::plan::ControlLoopMode::Heartbeat;
    let owned = seed(store, owned_plan);
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

    let views =
        codeterm_service_core::delayed_send::system_heartbeat_views(&fx.service, &orchestrator)
            .unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(
        views[0].message.id,
        format!("system:heartbeat:{owned}:{orchestrator}")
    );
    assert_eq!(views[0].message.tab_id, orchestrator);
    assert_eq!(
        views[0].message.origin,
        codeterm_service_core::delayed_send::MessageOrigin::System
    );
    assert!(!views[0].message.text.is_empty());
    assert!(store.plan_loads_for_test().is_empty());
    assert_never_loaded(store, &[&other]);
}

#[test]
fn reconcile_notification_questions_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let clock = codeterm_service_core::cron::clock::MockClock::starting_at(1_700_000_000_000);
    let cron = std::sync::Arc::new(parking_lot::Mutex::new(
        codeterm_service_core::cron::store::CronStore::new_with_path_and_clock(
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

    let mut projected = store.curator_schedule_reads_for_test();
    projected.sort();
    let mut matched = vec![active, scheduled];
    matched.sort();
    assert_eq!(projected, matched);
    assert_never_loaded(store, &[&quiet]);
}
