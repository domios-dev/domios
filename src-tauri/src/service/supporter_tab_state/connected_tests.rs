use super::*;

#[test]
fn supporter_tab_state_other_label_takes_no_action() {
    let model = StubModel::answering(Ok(choice(OTHER, 0.99)));
    let (host, calls, _) = StubHost::new("ambiguous fixture", model);
    let tab_id = tab();
    let verdict = run(&host, &tab_id, ObservationTrigger::WorkerIdle).unwrap();
    assert_eq!(verdict.label, OTHER);
    assert_eq!(verdict.action, SignalAction::None);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(host.deliveries.lock().is_empty());
    let status = host.runtime.status(600);
    assert_eq!(status["last_verdict"]["tab_state"]["label"], OTHER);
}

#[test]
fn supporter_tab_state_budget_skips_model_call_and_signal() {
    let model = StubModel::answering(Ok(choice(READY_IDLE, 0.99)));
    let (host, calls, _) = StubHost::new("healthy fixture", model);
    host.runtime
        .reserve(1, VerdictKind::Rerank, None, 1)
        .unwrap();
    let config = SupporterConfig {
        calls_per_hour: 1,
        ..enabled()
    };
    let verdict = spawn_observer(
        &host,
        &config,
        "fixture",
        3000,
        &tab(),
        ObservationTrigger::SpawnReady,
        "structural",
    )
    .unwrap()
    .join()
    .unwrap();
    assert!(verdict.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(host.deliveries.lock().is_empty());
}
