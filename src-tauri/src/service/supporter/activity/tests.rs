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
    runtime.verdict(
        VerdictKind::Rerank,
        None,
        &DecisionAnswer::Noul { p: 0.8 },
        Some("fixture-model".into()),
        "fixture-version",
        "fixture-hash",
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
