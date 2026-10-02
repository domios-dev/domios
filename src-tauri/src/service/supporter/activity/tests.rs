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
            model_id: Some("fixture-model".into()),
            criteria_version: "fixture-version",
            state_hash: "fixture-hash",
            question_set_hash: "fixture-question-hash",
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
            model_id: Some("reported".into()),
            criteria_version: "criteria",
            state_hash: "state-hash",
            question_set_hash: "questions-hash",
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
