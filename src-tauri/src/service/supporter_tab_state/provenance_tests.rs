use super::*;
use serde_json::json;

#[test]
fn supporter_first_opinion_passive_observer_retains_original_capture_without_extra_reads() {
    for trigger in [
        ObservationTrigger::SpawnReady,
        ObservationTrigger::SpawnDeadline,
        ObservationTrigger::StartupEscalation,
        ObservationTrigger::PromptCandidate,
        ObservationTrigger::WorkerIdle,
        ObservationTrigger::BusyNoOutput,
        ObservationTrigger::TabExit,
        ObservationTrigger::UserDeliveryReadiness,
    ] {
        let (host, calls, _) = StubHost::new(
            "fixture-frame",
            StubModel::answering(Ok(choice(OTHER, 0.95))),
        );
        let tab = tab();
        let (_, metadata) = prepare_observation(&host, &tab, trigger, "fixture").unwrap();
        let captured = metadata.envelope.as_ref().unwrap();
        observer::record(
            &host,
            &tab,
            identity::correlation::RecordIdentity::captured(
                captured.observation_id,
                &captured.correlation,
            ),
            identity::DecisionSource::Budget,
            OTHER,
            None,
            &metadata.title,
        );
        let lines = host.records.lock();
        let event: Value = serde_json::from_str(
            lines
                .last()
                .unwrap()
                .strip_prefix("[tab-state/decision] ")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(event["decision_point"], trigger.label());
        assert_eq!(
            event["correlation"]["observation_id"],
            captured.observation_id
        );
        assert_eq!(
            event["correlation"]["frame_hash"],
            format!("{:016x}", captured.frame_hash)
        );
        assert_eq!(
            event["correlation"]["candidate_evidence"],
            json!({"status":"unavailable"})
        );
        assert_eq!(event["observation_status"], "rejected");
        assert_eq!(host.reads.load(Ordering::SeqCst), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn supporter_first_opinion_passive_model_completion_and_transport_failure_are_distinct() {
    for (answer, source, status) in [
        (Ok(choice(OTHER, 0.95)), "fallback(other)", "completed"),
        (
            Ok(choice(PROVIDER_UPDATE_PROMPT, 0.4)),
            "fallback(low_confidence)",
            "completed",
        ),
        (
            Err(DecisionError::Deadline),
            "fallback(timeout)",
            "rejected",
        ),
    ] {
        let (host, _, _) = StubHost::new("fixture-frame", StubModel::answering(answer));
        observer::observe_guarded(
            &host,
            &enabled(),
            "fixture",
            3000,
            &tab(),
            ObservationTrigger::SpawnReady,
            "fixture",
        );
        let event = host
            .records
            .lock()
            .iter()
            .find_map(|line| {
                line.strip_prefix("[tab-state/decision] ")
                    .map(|body| serde_json::from_str::<Value>(body).unwrap())
            })
            .unwrap();
        assert_eq!(event["decided_by"], source);
        assert_eq!(event["observation_status"], status);
        assert_eq!(event["decision_point"], "launch_ready");
        assert!(event.get("correlation").is_some());
        assert_eq!(host.reads.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn supporter_first_opinion_passive_disconnected_producers_add_no_receipt_or_journal() {
    let (host, calls, _) = StubHost::new(
        "fixture-frame",
        StubModel::answering(Ok(choice(OTHER, 0.95))),
    );
    let config = SupporterConfig::default();
    let tab = tab();
    assert!(spawn_observer(
        &host,
        &config,
        "fixture",
        3000,
        &tab,
        ObservationTrigger::PromptCandidate,
        "fixture"
    )
    .is_none());
    assert!(
        candidates::spawn(&host, &config, &tab, None, |_, _, _| panic!(
            "disconnected callback"
        ))
        .is_none()
    );
    assert_eq!(host.reads.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(host.records.lock().is_empty());
    assert_eq!(
        std::fs::read_dir(host._journal_dir.path()).unwrap().count(),
        0
    );
}

#[test]
fn supporter_first_opinion_passive_candidate_fallback_keeps_only_the_model_answer_that_ran() {
    let (host, calls, _) = StubHost::new(
        "fixture-frame",
        StubModel::answering(Ok(choice(TRUST_OR_PERMISSION_PROMPT, 0.99))),
    );
    candidates::spawn(&host, &enabled(), &tab(), None, |verdict, source, _| {
        assert!(verdict.is_none());
        assert_eq!(source, identity::DecisionSource::Other);
    })
    .unwrap()
    .join()
    .unwrap();
    let journal = std::fs::read_to_string(host._journal_dir.path().join("journal.jsonl")).unwrap();
    let event = journal
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|event| event["action"] == "classification")
        .unwrap();
    assert_eq!(event["decided_by"], "fallback(other)");
    assert_eq!(event["label"], OTHER);
    assert_eq!(
        event["model_verdict"],
        json!({"status":"observed", "label":TRUST_OR_PERMISSION_PROMPT,"confidence":0.99})
    );
    assert_eq!(event["alternate_verdict"], json!({"status":"unavailable"}));
    assert_eq!(event["decision_point"], "prompt_candidate");
    assert_eq!(event["observation_status"], "completed");
    let feed = host.runtime.feed(100, None).unwrap();
    let projected = feed
        .iter()
        .find(|entry| entry["observation_id"] == event["observation_id"])
        .unwrap();
    assert_eq!(projected["correlation"], event["correlation"]);
    assert_eq!(projected["model_verdict"], event["model_verdict"]);
    assert_eq!(host.reads.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}
