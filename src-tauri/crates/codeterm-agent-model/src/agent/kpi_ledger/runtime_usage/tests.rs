use super::*;
use crate::agent::kpi_ledger::TokenCounts;

fn usage(response: &str, input: u64) -> KpiEventInput {
    KpiEventInput {
        plan_id: Some("plan".into()),
        task_id: Some("task".into()),
        tab_id: "tab".into(),
        role: Some("Worker".into()),
        provider: "runtime".into(),
        model: None,
        timestamp: 123,
        kind: KpiEventKind::TokenUsage,
        bytes: input + 3,
        tokens: Some(TokenCounts {
            input,
            uncached_input: input,
            output: 3,
        }),
        dedup_key: Some(format!("usage:runtime:{response}")),
        workspace_id: Some("workspace".into()),
        tab_kind: None,
    }
}

#[test]
fn runtime_usage_archive_replay_restart_and_rebuild_are_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let ledger = KpiLedger::open(root.path()).unwrap();
    ledger
        .record_runtime_usage(&usage("response-a", 10))
        .unwrap();
    ledger
        .record_runtime_usage(&usage("response-a", 20))
        .unwrap();
    ledger
        .record_runtime_usage(&usage("response-a", 5))
        .unwrap();
    ledger
        .record_runtime_usage(&usage("response-b", 0))
        .unwrap();
    let expected = ledger.list_all().unwrap();
    assert_eq!(expected.len(), 2);
    assert_eq!(
        expected
            .iter()
            .map(|row| row.tokens.unwrap().input)
            .sum::<u64>(),
        20
    );
    drop(ledger);
    let ledger = KpiLedger::open(root.path()).unwrap();
    let rebuilt = ledger.rebuild_runtime_usage().unwrap();
    assert_eq!(rebuilt.responses, 2);
    assert!(rebuilt.plans.contains("plan"));
    ledger.rebuild_runtime_usage().unwrap();
    assert_eq!(ledger.list_all().unwrap(), expected);
    ledger
        .conn
        .lock()
        .execute("DELETE FROM kpi_events WHERE kind = 'token_usage'", [])
        .unwrap();
    ledger.rebuild_runtime_usage().unwrap();
    let recovered = ledger.list_all().unwrap();
    assert_eq!(recovered.len(), 2);
    assert_eq!(
        recovered
            .iter()
            .map(|row| row.tokens.unwrap().input)
            .sum::<u64>(),
        20
    );
    assert!(recovered
        .iter()
        .all(|row| row.task_id.as_deref() == Some("task")));
}

#[test]
fn runtime_usage_archive_refuses_unknown_usage_and_foreign_owners() {
    let root = tempfile::tempdir().unwrap();
    let ledger = KpiLedger::open(root.path()).unwrap();
    let mut unknown = usage("unknown", 0);
    unknown.tokens = None;
    assert!(ledger.record_runtime_usage(&unknown).is_err());
    assert!(ledger.list_all().unwrap().is_empty());
    ledger.remember_usage_provider("source", "owner").unwrap();
    assert!(ledger.remember_usage_provider("source", "other").is_err());
    assert_eq!(
        ledger.usage_provider("source").unwrap().as_deref(),
        Some("owner")
    );
    assert_eq!(ledger.usage_provider("unseen").unwrap(), None);
}
