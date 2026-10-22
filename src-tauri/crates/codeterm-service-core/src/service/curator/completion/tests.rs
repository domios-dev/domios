use super::*;

#[test]
fn manual_close_settles_a_partial_round_once_without_triggering_failure_fallback() {
    let (mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    let mut plan = Plan::new(
        "Fixture".into(),
        fixture.temp.path().display().to_string(),
        Default::default(),
    );
    plan.control_loop.running = Some(CuratorRun {
        tab_id: Some(tab.clone()),
        run_id: Some("run".into()),
        number: 2,
        started_at: 100,
        ..Default::default()
    });
    service.plan_store.save(&mut plan, 100).unwrap();
    for _ in 0..2 {
        let round = service
            .settle_curator_run_end(&plan.id, &tab, "run", 101, RunEnd::TabClosed)
            .unwrap();
        assert_eq!(round.outcome, codeterm_types::plan::CuratorOutcome::Partial);
        assert_eq!(round.reason, "tab_closed");
    }
    let plan = service.plan_store.get(&plan.id).unwrap();
    assert!(plan.control_loop.running.is_none());
    assert_eq!(plan.control_loop.consecutive_failures, 0);
    assert!(plan.control_loop.fallback_reason.is_none());
    assert_eq!(plan.control_loop.last_real_round_at, Some(101));
}
