use super::*;
use serde_json::json;

fn origin(ledger: &OperationLedger) -> JobOriginReceipt {
    ledger
        .admit_job_origin(
            PluginJobScope {
                plugin: "fixture".into(),
                workspace: "workspace".into(),
                team: None,
                owner: "owner".into(),
            },
            PluginJobOrigin::StudioAgent {
                surface: "studio".into(),
                instance: "one".into(),
                tab: "surface-tab".into(),
                actor: "actor".into(),
            },
            100,
        )
        .unwrap()
}
fn quote(request: &serde_json::Value) -> JobQuote {
    JobQuote {
        binding: job_request_binding(request).unwrap(),
        amount: 2,
        unit: "credits".into(),
        basis: "account tariff".into(),
        tariff_revision: "revision".into(),
        issued_at: 100,
        valid_until_ms: 1000,
    }
}
#[test]
fn exact_job_quote_requires_owner_and_entry_is_durable_and_never_replayed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ledger.sqlite");
    let ledger = OperationLedger::open(&path).unwrap();
    let origin = origin(&ledger);
    let request = json!({"revision":1,"params":{"prompt":"opaque"},"inputs":[]});
    let price = quote(&request);
    assert_eq!(
        ledger
            .prepare_job_spend(&origin, &request, price.clone(), 100)
            .unwrap()
            .state,
        JobSpendState::WaitingForOwner
    );
    assert!(ledger.enter_job_spend(&origin, &request, 100).is_err());
    ledger
        .decide_job_spend("fixture", &origin.operation, "Approve once", 100)
        .unwrap();
    assert!(ledger
        .enter_job_spend(&origin, &json!({"revision":2}), 100)
        .is_err());
    let mut foreign = origin.clone();
    foreign.scope.workspace = "foreign".into();
    assert!(ledger.enter_job_spend(&foreign, &request, 100).is_err());
    foreign = origin.clone();
    if let PluginJobOrigin::StudioAgent { actor, .. } = &mut foreign.origin {
        *actor = "foreign".into();
    }
    assert!(ledger.enter_job_spend(&foreign, &request, 100).is_err());
    assert!(ledger.enter_job_spend(&origin, &request, 1000).is_err());
    let entered = ledger.enter_job_spend(&origin, &request, 101).unwrap();
    assert_eq!(entered.state, JobSpendState::EnteredUnknown);
    drop(ledger);
    let ledger = OperationLedger::open(&path).unwrap();
    assert_eq!(
        ledger.job_spend("fixture", &origin.operation).unwrap(),
        Some(entered)
    );
    assert!(ledger.enter_job_spend(&origin, &request, 102).is_err());
    assert!(ledger
        .decide_job_spend("fixture", &origin.operation, "Approve once", 102)
        .is_err());
    assert!(ledger
        .prepare_job_spend(&origin, &request, JobQuote { amount: 1, ..price }, 102)
        .is_err());
}
#[test]
fn allowance_auto_approves_same_unit_and_retains_unknown_reservations() {
    let root = tempfile::tempdir().unwrap();
    let ledger = OperationLedger::open(&root.path().join("ledger.sqlite")).unwrap();
    let request = json!({"revision":1});
    let initial = origin(&ledger);
    ledger
        .prepare_job_spend(&initial, &request, quote(&request), 100)
        .unwrap();
    ledger
        .decide_job_spend("fixture", &initial.operation, "Set daily allowance", 100)
        .unwrap();
    for _ in 0..10 {
        let next = origin(&ledger);
        assert_eq!(
            ledger
                .prepare_job_spend(&next, &request, quote(&request), 100)
                .unwrap()
                .state,
            JobSpendState::ApprovedBudget
        );
        ledger.enter_job_spend(&next, &request, 100).unwrap();
    }
    assert!(ledger.enter_job_spend(&initial, &request, 100).is_err());
    let next = origin(&ledger);
    assert_eq!(
        ledger
            .prepare_job_spend(&next, &request, quote(&request), 100)
            .unwrap()
            .state,
        JobSpendState::WaitingForOwner
    );
    let other = origin(&ledger);
    let mut other_quote = quote(&request);
    other_quote.unit = "USD".into();
    assert_eq!(
        ledger
            .prepare_job_spend(&other, &request, other_quote, 100)
            .unwrap()
            .state,
        JobSpendState::WaitingForOwner
    );
}
#[test]
fn quote_bounds_and_canonical_binding_are_closed() {
    let request = json!({"revision":1,"params":{"a":2,"z":3}});
    assert_eq!(
        job_request_binding(&request).unwrap(),
        job_request_binding(&json!({"params":{"z":3,"a":2},"revision":1})).unwrap()
    );
    let mut price = quote(&request);
    assert!(price.valid(100));
    price.valid_until_ms = 300_101;
    assert!(!price.valid(100));
    assert!(job_request_binding(&json!({"params":"x".repeat(2049)})).is_err());
    assert!(job_request_binding(&json!("label")).is_err());
}

#[test]
fn concurrent_entries_cannot_oversubscribe_one_host_allowance() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ledger.sqlite");
    let ledger = OperationLedger::open(&path).unwrap();
    let request = json!({"revision":1});
    let initial = origin(&ledger);
    ledger
        .prepare_job_spend(&initial, &request, quote(&request), 100)
        .unwrap();
    ledger
        .decide_job_spend("fixture", &initial.operation, "Set daily allowance", 100)
        .unwrap();
    let origins: Vec<_> = (0..20)
        .map(|_| {
            let next = origin(&ledger);
            ledger
                .prepare_job_spend(&next, &request, quote(&request), 100)
                .unwrap();
            next
        })
        .collect();
    let threads: Vec<_> = origins
        .into_iter()
        .map(|origin| {
            let ledger = OperationLedger::open(&path).unwrap();
            let request = request.clone();
            std::thread::spawn(move || ledger.enter_job_spend(&origin, &request, 100).is_ok())
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .filter(|entered| *entered)
            .count(),
        10
    );
}
