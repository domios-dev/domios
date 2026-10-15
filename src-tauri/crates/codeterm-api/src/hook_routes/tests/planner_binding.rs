use super::*;

#[tokio::test]
async fn planner_pty_session_claim_finalization_records_only_verified_registered_tab() {
    use codeterm_service_core::plan::{OrchestratorRef, Plan, PlannerRef};
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    let (mux, tab_id) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fx = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "spawned planner".into(),
        fx.temp.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.planner = Some(PlannerRef::Tab {
        tab_id: tab_id.clone(),
        session_id: None,
        title: None,
    });
    fx.service.plan_store.save(&mut plan, 1).unwrap();
    {
        let mut mux = fx.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("own-session".into());
        tab.session_binding_nonce = Some("own-nonce".into());
        tab.reported_session_binding_nonce = Some("stale-nonce".into());
    }
    bind_planner_session_after_claim(&fx.service, &tab_id)
        .await
        .unwrap();
    assert_eq!(
        fx.service.plan_store.get(&plan.id).unwrap().planner,
        plan.planner
    );
    fx.service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .reported_session_binding_nonce = Some("own-nonce".into());
    bind_planner_session_after_claim(&fx.service, &tab_id)
        .await
        .unwrap();
    assert!(
        matches!(fx.service.plan_store.get(&plan.id).unwrap().planner,
        Some(PlannerRef::Tab { session_id: Some(session), .. }) if session == "own-session")
    );
}
