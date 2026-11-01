use super::*;
use std::sync::mpsc::RecvTimeoutError;

pub(super) fn fixture_clock() -> deadline::FixtureClock {
    deadline::FixtureClock::new(Instant::now() - Duration::from_secs(60))
}

#[test]
fn decision_fixture_clock_preserves_graph_and_fallback_after_persistence() {
    let clock = fixture_clock();
    let origin = clock.now();
    let _scope = clock.enter();
    assert!(origin + Duration::from_secs(3) < Instant::now());
    let backend = graph::graph_seeded();
    let journal_path = backend._journal_dir.path().join("journal.jsonl");
    let persistence_events = Arc::new(AtomicUsize::new(0));
    let observed = persistence_events.clone();
    let during_persistence = clock.clone();
    backend.runtime.attach_visibility(
        Arc::new(|_| None),
        Arc::new(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            during_persistence.advance_to(origin + Duration::from_secs(1));
        }),
    );
    let output = resolve_search(
        &backend,
        &relevance_registry(&[(graph::GRAPH_TITLES[0], 0.99)]),
        &graph::graph_cfg(3),
        graph::applied(50),
    )
    .unwrap();
    assert!(persistence_events.load(Ordering::SeqCst) > 0);
    let ids = returned_mem_ids(&output);
    assert_eq!(ids.len(), 7);
    assert_eq!(output["hits"].as_array().unwrap().len(), 7);
    assert_eq!(titles(&output)[0], graph::GRAPH_TITLES[0]);
    assert_eq!(output["hits"][0]["via"], "graph");
    assert_eq!(output["supporter"]["status"], "applied");
    assert_eq!(
        output["supporter"]["sources"],
        json!({ "recall": 4, "graph": 3 })
    );
    let journal = std::fs::read_to_string(journal_path).unwrap();
    let rows: Vec<Value> = journal
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let first_spend = rows
        .iter()
        .position(|row| row["kind"] == "spend_updated")
        .unwrap();
    let first_verdict = rows
        .iter()
        .position(|row| row["action"] == "verdict")
        .unwrap();
    let last_verdict = rows
        .iter()
        .rposition(|row| row["action"] == "verdict")
        .unwrap();
    assert!(first_spend < first_verdict);
    assert_eq!(first_verdict, last_verdict);
    assert_eq!(rows.iter().filter(|row| row["kind"] == "rerank").count(), 1);
    assert_eq!(rows[first_verdict]["question_count"], 7);
    assert_eq!(persistence_events.load(Ordering::SeqCst), rows.len());
    let request = params(2);
    for (reason, error) in [
        ("adapter_error", DecisionError::Http(500)),
        ("deadline", DecisionError::Deadline),
        ("budget", DecisionError::Budget),
        ("unavailable", DecisionError::Unavailable("opaque".into())),
    ] {
        let baseline = baseline_oracle(&backend, &request);
        let (registry, completion) = registry_with_completion(StubMode::Fail(error));
        let fallback = resolve_search(
            &backend,
            &registry,
            &graph::graph_cfg(3),
            SearchParams {
                supporter: Some(true),
                ..request.clone()
            },
        )
        .unwrap();
        finish_registry(registry, completion);
        assert_eq!(
            fallback["supporter"],
            json!({ "status": "fallback", "reason": reason })
        );
        assert_eq!(normalized(without_envelope(fallback)), normalized(baseline));
    }
}

#[test]
fn decision_real_deadline_fallback_hits_equal_baseline() {
    let (_held_sender, signal) = mpsc::channel();
    let backend = seeded();
    let (registry, completion) = registry_with_completion(StubMode::Gate {
        signal: Mutex::new(signal),
        released: AtomicBool::new(false),
    });
    let mut request = params(2);
    request.project = Some("/seeded/project".into());
    let baseline = baseline_oracle(&backend, &request);
    let output = resolve_search(
        &backend,
        &registry,
        &SupporterConfig {
            deadline_ms: 100,
            ..enabled_cfg()
        },
        SearchParams {
            supporter: Some(true),
            ..request
        },
    )
    .unwrap();
    finish_registry(registry, completion);
    assert_eq!(
        normalized(json!({ "hits": output["hits"] })),
        normalized(json!({ "hits": baseline["hits"] })),
    );
    assert_eq!(
        output["supporter"],
        json!({ "status": "fallback", "reason": "deadline" }),
    );
    assert_eq!(normalized(without_envelope(output)), normalized(baseline));
}

#[test]
fn decision_real_deadline_receiver_expires_after_worker_entry_without_joining() {
    let (response, receiver) = deadline::channel();
    assert!(matches!(receiver, deadline::Receiver::Real(_)));
    let (release, signal) = mpsc::channel();
    let (entered, entry) = mpsc::channel();
    let (finished, completion) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        entered.send(()).unwrap();
        signal
            .recv_timeout(Duration::from_secs(10))
            .expect("fixture_model_release_liveness_exceeded");
        finished.send(response.send(17)).unwrap();
    });
    entry.recv_timeout(Duration::from_secs(10)).unwrap();
    let limit = Instant::now() + Duration::from_millis(100);
    assert_eq!(receiver.recv_until(limit), Err(RecvTimeoutError::Timeout));
    assert!(Instant::now() >= limit);
    assert_eq!(completion.try_recv(), Err(mpsc::TryRecvError::Empty));
    drop(receiver);
    release.send(()).unwrap();
    assert_eq!(
        completion
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap_err()
            .0,
        17,
    );
    worker.join().unwrap();
}

#[test]
fn decision_fixture_clock_shared_state_keeps_all_candidates() {
    let _scope = fixture_clock().enter();
    let backend = graph::graph_seeded();
    let mut cfg = graph::graph_cfg(3);
    cfg.memory_rerank.request_mode = codeterm_types::config::RerankRequestMode::SharedState;
    let output =
        resolve_search(&backend, &relevance_registry(&[]), &cfg, graph::applied(50)).unwrap();
    assert_eq!(returned_mem_ids(&output).len(), 7);
    assert_eq!(output["hits"].as_array().unwrap().len(), 7);
    assert_eq!(output["supporter"]["status"], "applied");
    assert_eq!(
        output["supporter"]["sources"],
        json!({ "recall": 4, "graph": 3 })
    );
    assert!(output["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit["relevance"] == 0.0));
}
