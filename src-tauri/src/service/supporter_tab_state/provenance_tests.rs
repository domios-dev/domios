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
