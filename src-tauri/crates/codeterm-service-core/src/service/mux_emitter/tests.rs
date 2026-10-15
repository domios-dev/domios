use super::*;

#[test]
fn burst_emits_once_with_last_generation_and_identical_published_bytes() {
    let (emitter, receiver) = Emitter::channel();
    let pending = emitter.pending.clone();
    for generation in 1..=100 {
        assert!(emitter.request(generation, None).is_none());
    }
    drop(emitter);
    let published = PublishedMuxState::new(0, &serde_json::json!({"generation": 0})).unwrap();
    let mut emissions = Vec::new();
    run(
        receiver,
        pending,
        |generation, snapshot_requested, focuses| {
            assert!(snapshot_requested);
            assert!(focuses.is_empty());
            let snapshot = serde_json::json!({"generation": generation});
            assert!(published.publish(generation, &snapshot).unwrap());
            assert_eq!(
                published.read().as_ref(),
                serde_json::to_vec(&snapshot).unwrap()
            );
            emissions.push(generation);
        },
    );
    assert_eq!(emissions, [100]);
}

#[test]
fn mutation_during_emission_produces_one_trailing_emission() {
    let (emitter, receiver) = Emitter::channel();
    let pending = emitter.pending.clone();
    assert!(emitter.request(1, None).is_none());
    let mut emitter = Some(emitter);
    let mut emissions = Vec::new();
    run(receiver, pending, |generation, _, _| {
        emissions.push(generation);
        if let Some(emitter) = emitter.take() {
            for generation in 2..=100 {
                assert!(emitter.request(generation, None).is_none());
            }
        }
    });
    assert_eq!(emissions, [1, 100]);
}

#[test]
fn focus_is_emitted_after_snapshot_of_committed_focus() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;

    let (mut state, tab_id) = MultiplexerState::new();
    let workspace = state.create_workspace("second".into(), None).id;
    let target = state.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(state);
    let service = &fixture.service;
    let (emitter, receiver) = Emitter::channel();
    let pending = emitter.pending.clone();
    assert!(emitter.request(service.clone(), None).is_none());
    crate::service::tab::focus_tab_hot_path(
        &service.mux,
        &target,
        |focused| {
            assert_ne!(focused, tab_id);
            assert_eq!(service.mux.read().get_focused_tab_id(), Some(focused));
            assert!(emitter
                .request(service.clone(), Some(focused.to_owned()))
                .is_none());
        },
        || {},
    )
    .unwrap();
    drop(emitter);
    run(receiver, pending, emit);
    let events = fixture.ui.0.lock();
    let structural = events
        .iter()
        .position(|(name, _)| name == "mux-state-changed")
        .unwrap();
    let focused = events
        .iter()
        .position(|(name, _)| name == "tab-focused")
        .unwrap();
    assert!(structural < focused);
    assert_eq!(events[focused].1, serde_json::json!({"tab_id": target}));
    assert_eq!(events[structural].1["focused_tab_id"], target);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&service.published_mux_state()).unwrap(),
        events[structural].1,
    );
}

#[test]
fn disconnected_emitter_returns_unpublished_state_for_recovery() {
    let (emitter, receiver) = Emitter::channel();
    drop(receiver);
    assert_eq!(
        emitter.request(7, Some("tab".into())),
        Some((7, false, vec!["tab".into()]))
    );
    assert!(emitter.pending.lock().latest.is_none());
}

#[test]
fn idle_emitter_wakes_on_request_and_exits_when_owner_drops() {
    use std::time::Duration;

    let (emitter, receiver) = Emitter::channel();
    let pending = emitter.pending.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (emitted_tx, emitted_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        run(receiver, pending, |generation, _, _| {
            emitted_tx.send(generation).unwrap();
        });
    });
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(emitter.request(42, None).is_none());
    assert_eq!(emitted_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 42);
    drop(emitter);
    worker.join().unwrap();
}

#[test]
fn focus_only_burst_preserves_every_focus_without_requesting_snapshot() {
    let (emitter, receiver) = Emitter::channel();
    let pending = emitter.pending.clone();
    assert!(emitter.request(1, Some("first".into())).is_none());
    assert!(emitter.request(2, Some("second".into())).is_none());
    drop(emitter);
    let mut emissions = Vec::new();
    run(
        receiver,
        pending,
        |generation, snapshot_requested, focuses| {
            assert!(!snapshot_requested);
            emissions.push((generation, focuses));
        },
    );
    assert_eq!(emissions, [(2, vec!["first".into(), "second".into()])]);
}
