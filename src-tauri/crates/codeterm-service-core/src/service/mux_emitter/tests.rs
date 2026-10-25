use super::*;

#[test]
fn status_delta_fanout_stays_small_with_221_tabs() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    let (mut state, tab_id) = MultiplexerState::new();
    state.set_limit_gate(codeterm_entitlements::LimitGate::dormant());
    let workspace = state.list_workspaces()[0].id.clone();
    for _ in 1..221 {
        let id = state.create_tab(&workspace).unwrap();
        state.rename_tab(&id, &"x".repeat(2800)).unwrap();
    }
    state.get_tab_mut(&tab_id).unwrap().pty_id = Some("delta-pty".into());
    let fixture = ServiceFixture::new(state);
    let service = &fixture.service;
    service.agent_tracker.lock().set_working("delta-pty");
    let base = service.published_mux_state_versioned().0;
    let mut bus = service.events.subscribe();
    emit(
        service.clone(),
        true,
        Vec::new(),
        MuxEmitCause::TabStatus.bit(),
    );
    let event = bus.try_recv().unwrap();
    let codeterm_events::SystemEvent::Generic { event_type, data } = event.event else {
        panic!("expected mux delta");
    };
    assert_eq!(event_type, "mux-state-changed");
    assert_eq!(data["base_generation"], base);
    assert_eq!(
        data["status_deltas"],
        serde_json::json!({
            tab_id: {"status": "Working", "agent_state": null}
        })
    );
    assert!(serde_json::to_vec(&data).unwrap().len() < 512);
    assert!(service.published_mux_state().len() > 570_000);
    let native = fixture.ui.0.lock();
    let (_, signal) = native
        .iter()
        .find(|(name, _)| name == "mux-state-changed")
        .unwrap();
    assert_eq!(
        signal,
        &serde_json::json!({"generation": data["generation"]})
    );
    assert!(serde_json::to_vec(signal).unwrap().len() < 128);
}

#[test]
fn burst_emits_once_with_last_generation_and_identical_published_bytes() {
    let (emitter, receiver) = Emitter::channel();
    let pending = emitter.pending.clone();
    for generation in 1..=100 {
        assert!(emitter
            .request(generation, None, Some(MuxEmitCause::TabStatus))
            .is_none());
    }
    drop(emitter);
    let published = PublishedMuxState::new(0, &serde_json::json!({"generation": 0})).unwrap();
    let mut emissions = Vec::new();
    run(
        receiver,
        pending,
        |generation, snapshot_requested, focuses, _| {
            assert!(snapshot_requested);
            assert!(focuses.is_empty());
            let snapshot = serde_json::json!({"generation": generation, "title": "changed"});
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
    assert!(emitter
        .request(1, None, Some(MuxEmitCause::TabStatus))
        .is_none());
    let mut emitter = Some(emitter);
    let mut emissions = Vec::new();
    run(receiver, pending, |generation, _, _, _| {
        emissions.push(generation);
        if let Some(emitter) = emitter.take() {
            for generation in 2..=100 {
                assert!(emitter
                    .request(generation, None, Some(MuxEmitCause::TabStatus))
                    .is_none());
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
    assert!(emitter
        .request(service.clone(), None, Some(MuxEmitCause::TabStatus))
        .is_none());
    crate::service::tab::focus_tab_hot_path(
        &service.mux,
        &target,
        |focused| {
            assert_ne!(focused, tab_id);
            assert_eq!(service.mux.read().get_focused_tab_id(), Some(focused));
            assert!(emitter
                .request(service.clone(), Some(focused.to_owned()), None)
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
    let snapshot: serde_json::Value =
        serde_json::from_slice(&service.published_mux_state()).unwrap();
    assert_eq!(snapshot["focused_tab_id"], target);
    assert_eq!(
        events[structural].1,
        serde_json::json!({"generation": snapshot["generation"]}),
    );
}

#[test]
fn unchanged_snapshot_emits_nothing_and_a_change_emits_once() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;

    let (state, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(state);
    let service = &fixture.service;
    let status = MuxEmitCause::TabStatus.bit();
    emit(service.clone(), true, Vec::new(), status);
    let mut bus = service.events.subscribe();
    let ui_signals = || {
        fixture
            .ui
            .0
            .lock()
            .iter()
            .filter(|(name, _)| name == "mux-state-changed")
            .count()
    };
    let settled = ui_signals();
    let unchanged = || {
        service
            .ext::<Arc<MuxEmitStats>>()
            .unchanged
            .load(Ordering::Relaxed)
    };
    let unchanged_before = unchanged();
    for _ in 0..5 {
        emit(service.clone(), true, Vec::new(), status);
    }
    assert_eq!(ui_signals(), settled);
    service.mux.write().rename_tab(&tab_id, "Renamed").unwrap();
    emit(service.clone(), true, Vec::new(), MuxEmitCause::Title.bit());
    emit(service.clone(), true, Vec::new(), status);
    assert_eq!(ui_signals(), settled + 1);
    let mut bus_signals = Vec::new();
    while let Ok(event) = bus.try_recv() {
        if event.frame.event_type == "mux-state-changed" {
            bus_signals.push(event.event);
        }
    }
    assert!(matches!(
        bus_signals.as_slice(),
        [codeterm_events::SystemEvent::Generic { data, .. }]
            if data["generation"] == service.published_mux_state_versioned().0
    ));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&service.published_mux_state()).unwrap()
            ["tabs"][&tab_id]["title"],
        "Renamed"
    );
    assert_eq!(unchanged(), unchanged_before + 6);
}

#[test]
fn disconnected_emitter_returns_unpublished_state_for_recovery() {
    let (emitter, receiver) = Emitter::channel();
    drop(receiver);
    assert_eq!(
        emitter.request(7, Some("tab".into()), None),
        Some((7, false, vec!["tab".into()], 0))
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
        run(receiver, pending, |generation, _, _, _| {
            emitted_tx.send(generation).unwrap();
        });
    });
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(emitter
        .request(42, None, Some(MuxEmitCause::TabStatus))
        .is_none());
    assert_eq!(emitted_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 42);
    drop(emitter);
    worker.join().unwrap();
}

#[test]
fn focus_only_burst_preserves_every_focus_without_requesting_snapshot() {
    let (emitter, receiver) = Emitter::channel();
    let pending = emitter.pending.clone();
    assert!(emitter.request(1, Some("first".into()), None).is_none());
    assert!(emitter.request(2, Some("second".into()), None).is_none());
    drop(emitter);
    let mut emissions = Vec::new();
    run(
        receiver,
        pending,
        |generation, snapshot_requested, focuses, _| {
            assert!(!snapshot_requested);
            emissions.push((generation, focuses));
        },
    );
    assert_eq!(emissions, [(2, vec!["first".into(), "second".into()])]);
}

#[test]
fn mixed_cause_burst_counts_requests_and_attributes_one_fire_to_both() {
    let stats = MuxEmitStats::default();
    let (emitter, receiver) = Emitter::channel();
    let pending = emitter.pending.clone();
    for cause in [
        MuxEmitCause::Title,
        MuxEmitCause::TabStatus,
        MuxEmitCause::Title,
    ] {
        stats.causes.request(cause);
        stats.requests.fetch_add(1, Ordering::Relaxed);
        assert!(emitter.request(1, None, Some(cause)).is_none());
    }
    drop(emitter);
    run(receiver, pending, |_, snapshot_requested, _, causes| {
        assert!(snapshot_requested);
        stats.fires.fetch_add(1, Ordering::Relaxed);
        stats.causes.fire(causes);
    });
    let counts = stats.causes.snapshot();
    assert_eq!(stats.requests.load(Ordering::Relaxed), 3);
    assert_eq!(stats.fires.load(Ordering::Relaxed), 1);
    assert_eq!(counts[&MuxEmitCause::Title].requests, 2);
    assert_eq!(counts[&MuxEmitCause::TabStatus].requests, 1);
    assert_eq!(counts[&MuxEmitCause::Title].fires, 1);
    assert_eq!(counts[&MuxEmitCause::TabStatus].fires, 1);
    assert_eq!(counts[&MuxEmitCause::Cwd].requests, 0);
    assert_eq!(counts[&MuxEmitCause::Cwd].fires, 0);
}

#[test]
fn trailing_batch_does_not_inherit_previous_causes() {
    let (emitter, receiver) = Emitter::channel();
    let pending = emitter.pending.clone();
    emitter.request(1, None, Some(MuxEmitCause::Title));
    let mut emitter = Some(emitter);
    let mut batches = Vec::new();
    run(receiver, pending, |_, _, _, causes| {
        batches.push(causes);
        if let Some(emitter) = emitter.take() {
            emitter.request(2, None, Some(MuxEmitCause::Cwd));
        }
    });
    assert_eq!(
        batches,
        [MuxEmitCause::Title.bit(), MuxEmitCause::Cwd.bit()]
    );
}
