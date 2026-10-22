use super::*;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn explicit_finish_round_stays_alive_across_question_and_answer_turns() {
    let (mut mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    let target = mux.get_tab_mut(&tab).unwrap();
    target.role = Some(crate::multiplexer::types::AgentRole::Manager);
    target.role_profile = Some(
        crate::service::role_policy::RoleProfile::Curator
            .as_str()
            .into(),
    );
    target.session_binding_nonce = Some("own-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let registry = RunWatcherRegistry::new();
    let state = registry.insert_run_for_test("fixture-pty", &tab);
    registry.runs.lock().remove("fixture-pty");
    let mut state = Arc::try_unwrap(state).ok().unwrap();
    state.kind = AgentRunKind::Cron {
        job_id: "job".into(),
        run_id: "run".into(),
    };
    let state = Arc::new(state);
    registry
        .runs
        .lock()
        .insert("fixture-pty".into(), state.clone());
    for _ in 0..2 {
        registry.complete_via_stop_report(&fixture.service, &tab, "own-nonce");
        registry.complete_via_stop_hook(&fixture.service, &tab);
        registry.feed(
            &fixture.service,
            "fixture-pty",
            b"{\"type\":\"result\",\"is_error\":false,\"result\":\"opaque-turn\"}",
        );
        assert!(!state.completed.load(Ordering::Acquire));
        assert!(registry.runs.lock().contains_key("fixture-pty"));
        assert!(fixture.service.mux.read().get_tab(&tab).is_some());
    }
}
