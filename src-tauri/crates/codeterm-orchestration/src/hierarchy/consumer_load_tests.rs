use crate::test_fixture::ServiceFixture;
use crate::traits::HierarchyPlannerDeliveryExt;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::plan::load_audit::{
    assert_never_loaded, execution_plan, planner, seed, start,
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
