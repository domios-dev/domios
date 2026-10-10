use super::*;

fn lines(dir: &tempfile::TempDir) -> Vec<Value> {
    std::fs::read_to_string(dir.path().join("journal.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn supporter_budget_transition_journaled_once() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    runtime
        .reserve_at(2, VerdictKind::Rerank, None, 5, 100000)
        .unwrap();
    runtime
        .reserve_at(2, VerdictKind::TabState, Some("tab"), 2, 101000)
        .unwrap();
    for _ in 0..10 {
        assert_eq!(
            runtime.reserve_at(2, VerdictKind::Rerank, None, 1, 102000),
            Err(DecisionError::Budget)
        );
    }
    assert_eq!(runtime.status_at(2, 102000)["budget"]["free_at"], 3700);
    assert_eq!(
        lines(&dir)
            .iter()
            .filter(|line| line["kind"] == "budget_transition")
            .count(),
        1
    );
    assert_eq!(runtime.status_at(2, 3700000)["budget"]["reached"], false);
    runtime.status_at(2, 3700000);
    assert_eq!(
        lines(&dir)
            .iter()
            .filter(|line| line["kind"] == "budget_transition")
            .count(),
        2
    );
    assert_eq!(runtime.status_at(2, 3700000)["calls_last_day"], 2);
    assert_eq!(runtime.status_at(2, 86501000)["calls_last_day"], 0);
}

#[test]
fn supporter_global_budget_reservation_is_atomic_across_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = std::sync::Arc::new(SupporterRuntime::new(dir.path().into()));
    let workers: Vec<_> = (0..16)
        .map(|index| {
            let runtime = runtime.clone();
            std::thread::spawn(move || {
                runtime.reserve_at(
                    7,
                    VerdictKind::TabState,
                    Some(&index.to_string()),
                    2,
                    100000,
                )
            })
        })
        .collect();
    let admitted = workers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap().ok())
        .count();
    assert_eq!(admitted, 7);
    assert_eq!(runtime.status_at(7, 101000)["calls_last_hour"], 7);
    assert_eq!(
        lines(&dir)
            .iter()
            .filter(|line| line["kind"] == "model_call")
            .count(),
        7
    );
}

#[test]
fn supporter_status_exposes_activity_and_budget() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let empty = runtime.status(10);
    assert_eq!(empty["calls_last_hour"], 0);
    assert_eq!(empty["calls_last_day"], 0);
    assert_eq!(empty["last_activity"], Value::Null);
    runtime.reserve(10, VerdictKind::Rerank, None, 1).unwrap();
    runtime.response(
        VerdictKind::Rerank,
        None,
        &DecisionResponse {
            answers: std::collections::BTreeMap::from([(
                "q".into(),
                DecisionAnswer::Noul { p: 0.8 },
            )]),
            usage: None,
        },
        DecisionEvidence {
            decision_point: None,
            model_id: Some("fixture-model".into()),
            criteria_version: "fixture-version",
            state_hash: "fixture-hash",
            question_set_hash: "fixture-question-hash",
            asked: None,
        },
    );
    let status = runtime.status(10);
    assert_eq!(status["calls_last_hour"], 1);
    assert_eq!(status["calls_last_day"], 1);
    assert!(status["last_activity"].is_i64());
    assert_eq!(status["last_verdict"]["rerank"]["confidence"], 0.8);
    assert_eq!(status["budget"]["reached"], false);
}

#[test]
fn supporter_budget_survives_runtime_restart() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    runtime
        .reserve_at(1, VerdictKind::Rerank, None, 1, 100000)
        .unwrap();
    drop(runtime);
    let restarted = SupporterRuntime::new(dir.path().into());
    assert_eq!(
        restarted.reserve_at(1, VerdictKind::TabState, Some("tab"), 1, 101000),
        Err(DecisionError::Budget)
    );
    assert_eq!(restarted.status_at(1, 101000)["calls_last_hour"], 1);
    assert_eq!(
        lines(&dir)
            .iter()
            .filter(|line| line["kind"] == "budget_transition")
            .count(),
        1
    );
    restarted
        .reserve_at(1, VerdictKind::TabState, Some("tab"), 1, 3700000)
        .unwrap();
    assert_eq!(restarted.status_at(1, 3700000)["calls_last_day"], 2);
}

#[test]
fn supporter_journal_hashes_recovery_option_values_without_losing_probabilities() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    runtime.response(
        VerdictKind::TabState,
        None,
        &DecisionResponse {
            answers: std::collections::BTreeMap::from([
                (
                    "label".into(),
                    DecisionAnswer::Choice {
                        choice: "other".into(),
                        probabilities: std::collections::BTreeMap::from([("other".into(), 0.9)]),
                        confidence: Some(0.9),
                    },
                ),
                (
                    "safe_choice".into(),
                    DecisionAnswer::Choice {
                        choice: "private raw screen option".into(),
                        probabilities: std::collections::BTreeMap::from([
                            ("private raw screen option".into(), 0.8),
                            ("abstain".into(), 0.2),
                        ]),
                        confidence: Some(0.8),
                    },
                ),
            ]),
            usage: None,
        },
        DecisionEvidence {
            decision_point: None,
            model_id: Some("reported".into()),
            criteria_version: "criteria",
            state_hash: "state-hash",
            question_set_hash: "questions-hash",
            asked: None,
        },
    );
    let text = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    assert!(!text.contains("private raw screen option"));
    let value: Value = serde_json::from_str(text.trim()).unwrap();
    let hash = super::super::criteria_hash(b"private raw screen option");
    assert_eq!(value["distribution"]["safe_choice"]["choice"], hash);
    assert_eq!(
        value["distribution"]["safe_choice"]["probabilities"][hash],
        0.8
    );
    assert_eq!(value["distribution"]["label"]["choice"], "other");
}

#[test]
fn supporter_default_no_cap_never_refuses_and_counts_every_request() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let cap = codeterm_types::config::SupporterConfig::default()
        .calls_per_hour
        .unwrap_or(0);
    assert_eq!(cap, 0);
    for index in 0..601 {
        runtime
            .reserve_at(cap, VerdictKind::Rerank, None, 5, 100000 + index)
            .unwrap();
    }
    let status = runtime.status_at(cap, 101000);
    assert_eq!(status["calls_last_hour"], 601);
    assert_eq!(status["calls_last_day"], 601);
    assert_eq!(status["budget"]["calls_per_hour"], Value::Null);
    assert_eq!(status["budget"]["free_at"], Value::Null);
    assert_eq!(status["budget"]["reached"], false);
    assert_eq!(
        lines(&dir)
            .iter()
            .filter(|line| line["kind"] == "model_call")
            .count(),
        601
    );
}

#[test]
fn supporter_feed_journal_records_team_id_at_append() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let team = Arc::new(Mutex::new(Some("team_aabbccdd".to_owned())));
    let lookup = team.clone();
    runtime.attach_visibility(Arc::new(move |_| lookup.lock().clone()), Arc::new(|_| {}));
    runtime.action(ActionRecord {
        kind: VerdictKind::TabState,
        tab: Some("tab"),
        label: "other",
        confidence: 0.9,
        action: super::super::Action::WeakHint,
        reason: "launch_ready",
        asked: None,
    });
    *team.lock() = None;
    assert_eq!(lines(&dir)[0]["team_id"], "team_aabbccdd");
    assert_eq!(runtime.feed(10, Some("team_aabbccdd")).unwrap().len(), 1);
    assert_eq!(runtime.feed(10, Some("team_55667788")).unwrap().len(), 0);
}

#[test]
fn supporter_feed_journal_append_emits_event_after_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let events = Arc::new(Mutex::new(Vec::new()));
    let received = events.clone();
    let path = dir.path().join("journal.jsonl");
    runtime.attach_visibility(
        Arc::new(|_| Some("team_aabbccdd".into())),
        Arc::new(move |event| {
            assert!(std::fs::metadata(&path).unwrap().len() > 0);
            received.lock().push(event);
        }),
    );
    runtime
        .reserve(0, VerdictKind::Rerank, Some("tab"), 3)
        .unwrap();
    assert_eq!(events.lock().len(), 1);
    assert_eq!(events.lock()[0]["team_id"], "team_aabbccdd");
    assert!(events.lock()[0].get("screen").is_none());
}

#[test]
fn supporter_feed_failed_append_emits_no_event() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("journal.jsonl")).unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let received = count.clone();
    runtime.attach_visibility(
        Arc::new(|_| None),
        Arc::new(move |_| {
            received.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }),
    );
    assert!(runtime
        .try_action(ActionRecord {
            kind: VerdictKind::Rerank,
            tab: None,
            label: "ranked",
            confidence: 0.8,
            action: super::super::Action::Rerank,
            reason: "all_candidates_scored",
            asked: None,
        })
        .is_err());
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn supporter_explicit_cap_can_be_removed_after_reaching_limit() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    runtime
        .reserve_at(1, VerdictKind::Rerank, None, 1, 100000)
        .unwrap();
    assert_eq!(
        runtime.reserve_at(1, VerdictKind::Rerank, None, 1, 100001),
        Err(DecisionError::Budget)
    );
    runtime
        .reserve_at(0, VerdictKind::Rerank, None, 1, 100002)
        .unwrap();
    assert_eq!(runtime.status_at(0, 100003)["budget"]["reached"], false);
}

/// WHAT: Expose both Sentinel verdict journal paths in the owner activity feed.
/// WHY: Hidden watch decisions make the spinal cord invisible; ORACLE: persisted verdicts survive safe projection.
#[test]
fn supporter_sentinel_verdict_is_visible_in_activity_feed() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    runtime.attach_visibility(Arc::new(|_| Some("team_aabbccdd".into())), Arc::new(|_| {}));
    runtime.response(
        VerdictKind::Sentinel,
        Some("private-creator"),
        &DecisionResponse {
            answers: std::collections::BTreeMap::from([(
                "bucket".into(),
                DecisionAnswer::Choice {
                    choice: "fixture-bucket".into(),
                    probabilities: Default::default(),
                    confidence: Some(0.95),
                },
            )]),
            usage: None,
        },
        DecisionEvidence {
            decision_point: None,
            model_id: Some("fixture-model".into()),
            criteria_version: "fixture-criteria",
            state_hash: "private-state",
            question_set_hash: "fixture-questions",
            asked: None,
        },
    );
    runtime
        .sentinel_event(json!({
            "time": chrono::Utc::now().timestamp(), "kind": "blueprint", "tab": "private-creator",
            "blueprint_id": "private-watch", "action": "decision", "reason": "transition",
            "label": "fixture-label", "confidence": 0.95, "state_hash": "private-state"
        }))
        .unwrap();
    assert_eq!(lines(&dir).len(), 2);
    let restored = SupporterRuntime::new(dir.path().into());
    let feed = restored.feed(10, Some("team_aabbccdd")).unwrap();
    assert_eq!(
        feed.len(),
        2,
        "Persisted Sentinel verdicts disappeared from the owner feed"
    );
    assert_eq!(feed[0]["action"], "decision");
    assert_eq!(feed[0]["reason"], "transition");
    assert_eq!(feed[1]["action"], "notify_only");
    assert_eq!(feed[1]["reason"], "model_answer");
    assert!(restored.feed(10, Some("team_11223344")).unwrap().is_empty());
    for entry in feed {
        assert_eq!(entry["kind"], "blueprint");
        assert_eq!(entry["team_id"], "team_aabbccdd");
        assert!(entry["time"].is_i64());
        assert_eq!(entry["label"], "redacted");
        assert_eq!(entry["confidence_band"], "strong");
        assert_eq!(entry["fallback"], false);
        assert!(entry.get("tab").is_none());
        assert!(entry.get("blueprint_id").is_none());
        assert!(entry.get("state_hash").is_none());
    }
}

#[test]
fn supporter_feed_sentinel_budget_preserves_safe_fields_and_redaction() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    runtime.attach_visibility(Arc::new(|_| Some("team_aabbccdd".into())), Arc::new(|_| {}));
    runtime
        .reserve_at(1, VerdictKind::Sentinel, Some("private-creator"), 1, 100000)
        .unwrap();
    runtime.sentinel_event(json!({
        "time": 100, "kind": "blueprint", "tab": "private-creator", "blueprint_id": "private-watch",
        "action": "decision", "reason": "budget_exhausted", "label": "private-bucket",
        "screen": "private-output", "token": "private-token", "state_hash": "private-state"
    })).unwrap();
    let restored = SupporterRuntime::new(dir.path().into());
    assert_eq!(restored.feed(10, Some("team_11223344")).unwrap().len(), 0);
    let feed = restored.feed(10, Some("team_aabbccdd")).unwrap();
    assert_eq!(
        feed,
        vec![json!({
            "time": 100, "team_id": "team_aabbccdd", "kind": "blueprint", "label": "redacted",
            "confidence_band": "unknown", "action": "decision", "decision_point": "blueprint", "reason": "budget_exhausted",
            "budget_reached": true, "fallback": true
        })]
    );
}
