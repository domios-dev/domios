use super::*;

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn unattributed_close_of_coordinator_reports_missing_attribution() {
    let (mut mux, coordinator) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&coordinator).expect("initial Tab");
    tab.role = Some(AgentRole::Manager);
    tab.role_profile = Some("orchestrator".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    let error = fixture
        .service
        .close_tab_guarded(None, &coordinator, false)
        .expect_err("an unattributed close of a coordinator must be refused");

    assert!(error.starts_with(UNATTRIBUTED_CLOSE_REFUSAL_PREFIX));
    assert!(!error.starts_with(SELF_CLOSE_REFUSAL_PREFIX));
    assert!(fixture.service.mux.read().get_tab(&coordinator).is_some());
}

#[test]
fn orchestrator_is_coordinator() {
    assert!(is_coordinator(
        Some(AgentRole::Manager),
        Some("orchestrator"),
        false,
        false
    ));
}

#[test]
fn plan_owner_is_coordinator() {
    assert!(is_coordinator(Some(AgentRole::Worker), None, true, false));
    assert!(is_coordinator(None, None, true, false));
}

#[test]
fn bound_planner_is_coordinator() {
    assert!(is_coordinator(Some(AgentRole::Worker), None, false, true));
}

#[test]
fn plain_worker_is_not_coordinator() {
    assert!(!is_coordinator(Some(AgentRole::Worker), None, false, false));
    assert!(!is_coordinator(None, None, false, false));
}
