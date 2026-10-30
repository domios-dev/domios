use super::*;

fn registration(operation: &str) -> PluginJobRegistration {
    PluginJobRegistration {
        scope: PluginJobScope {
            plugin: "fixture".into(),
            workspace: "workspace".into(),
            team: Some("team".into()),
            owner: "owner".into(),
        },
        origin: PluginJobOrigin::Blueprint {
            blueprint: "blueprint".into(),
            run: "run".into(),
            node: "node".into(),
            attempt: "attempt".into(),
        },
        operation: operation.into(),
        job: format!("job-{operation}"),
        next_check_ms: 1100,
        deadline_ms: 100_000,
    }
}

#[test]
fn plugin_monitor_exact_job_identity_exclusive_claim_and_cancel_are_durable() {
    let ledger = ledger();
    let registration = registration("monitor");
    ledger
        .monitor_existing_job(registration.clone(), 100)
        .unwrap();
    assert!(ledger.claim_job_check(1099).unwrap().is_none());
    let claim = ledger.claim_job_check(1100).unwrap().unwrap();
    assert!(ledger.claim_job_check(1100).unwrap().is_none());
    ledger
        .request_job_cancel("monitor", &registration.scope)
        .unwrap();
    let record = ledger
        .settle_job_check(
            claim,
            &registration.job,
            OperationState::Running,
            1200,
            1000,
        )
        .unwrap();
    assert_eq!(record.state, OperationState::Unknown);
    assert!(record.cancel_requested);
    assert_eq!(record.next_check_ms, 2200);
    assert_eq!(record.registration, registration);
    assert_eq!(
        ledger
            .monitor_existing_job(registration.clone(), 1200)
            .unwrap(),
        record
    );
    let claim = ledger.claim_job_check(2200).unwrap().unwrap();
    let record = ledger
        .settle_job_check(
            claim,
            &registration.job,
            OperationState::Canceled,
            2300,
            1000,
        )
        .unwrap();
    assert_eq!(record.state, OperationState::Canceled);
    assert!(ledger.next_job_check().unwrap().is_none());
    assert!(ledger
        .history(&request("unused").context.scope)
        .unwrap()
        .is_empty());
}

#[test]
fn plugin_monitor_stop_fences_late_completion_without_fabricating_provider_cancellation() {
    let ledger = ledger();
    let registration = registration("held");
    ledger
        .monitor_existing_job(registration.clone(), 100)
        .unwrap();
    let claim = ledger.claim_job_check(1100).unwrap().unwrap();
    ledger
        .stop_job_monitor("held", &registration.scope, MonitorStop::ScopeRemoved)
        .unwrap();
    assert!(ledger
        .settle_job_check(
            claim,
            &registration.job,
            OperationState::Succeeded,
            1200,
            1000
        )
        .is_err());
    let record = ledger.job_monitor("held").unwrap().unwrap();
    assert_eq!(record.stopped, Some(MonitorStop::ScopeRemoved));
    assert_eq!(record.state, OperationState::Unknown);
    assert_eq!(record.registration.job, "job-held");
    assert!(ledger.next_job_check().unwrap().is_none());
}

#[test]
fn plugin_monitor_rejects_replacement_jobs_and_scopes_and_bounds() {
    let ledger = ledger();
    let registration = registration("exact");
    ledger
        .monitor_existing_job(registration.clone(), 100)
        .unwrap();
    let mut changed = registration.clone();
    changed.job = "replacement".into();
    assert!(ledger.monitor_existing_job(changed, 100).is_err());
    let claim = ledger.claim_job_check(1100).unwrap().unwrap();
    assert!(ledger
        .settle_job_check(claim, "replacement", OperationState::Succeeded, 1200, 1000)
        .is_err());
    let mut scope = registration.scope.clone();
    scope.workspace = "foreign".into();
    assert!(ledger.request_job_cancel("exact", &scope).is_err());
    let mut too_soon = self::registration("too-soon");
    too_soon.next_check_ms = 101;
    assert!(ledger.monitor_existing_job(too_soon, 100).is_err());
}

#[test]
fn plugin_monitor_inflight_worker_excludes_same_plugin_after_persisted_lease_expiry() {
    let ledger = ledger();
    ledger
        .monitor_existing_job(registration("first"), 100)
        .unwrap();
    ledger
        .monitor_existing_job(registration("second"), 100)
        .unwrap();
    let claim = ledger.claim_job_check(1100).unwrap().unwrap();
    assert!(ledger.claim_job_check(32_000).unwrap().is_none());
    drop(claim);
    assert!(ledger.claim_job_check(32_000).unwrap().is_some());
}

#[test]
fn plugin_monitor_origin_receipt_survives_restart_and_retains_one_exact_job() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("jobs.sqlite");
    let registration = registration("origin");
    let receipt = {
        let ledger = OperationLedger::open(&path).unwrap();
        ledger
            .admit_job_origin(registration.scope, registration.origin, 100)
            .unwrap()
    };
    let ledger = OperationLedger::open(&path).unwrap();
    assert_eq!(
        ledger.job_origin("fixture", &receipt.operation).unwrap(),
        receipt
    );
    assert!(ledger.job_origin("foreign", &receipt.operation).is_err());
    let attached = ledger
        .attach_existing_job("fixture", &receipt.operation, "known-paid-job")
        .unwrap();
    assert_eq!(attached.job.as_deref(), Some("known-paid-job"));
    assert_eq!(
        ledger
            .attach_existing_job("fixture", &receipt.operation, "known-paid-job")
            .unwrap(),
        attached
    );
    assert!(ledger
        .attach_existing_job("fixture", &receipt.operation, "replacement")
        .is_err());
    assert_eq!(
        ledger
            .job_origin("fixture", &receipt.operation)
            .unwrap()
            .scope,
        receipt.scope
    );
}

#[test]
fn plugin_monitor_cancel_dispatch_is_once_and_an_orphan_only_reconciles() {
    let ledger = ledger();
    let registration = registration("cancel");
    ledger
        .monitor_existing_job(registration.clone(), 100)
        .unwrap();
    ledger
        .request_job_cancel("cancel", &registration.scope)
        .unwrap();
    let claim = ledger.claim_job_check(1100).unwrap().unwrap();
    assert!(claim.cancel);
    drop(claim);
    let claim = ledger.claim_job_check(32_000).unwrap().unwrap();
    assert!(!claim.cancel);
    assert_eq!(claim.monitor.registration.job, registration.job);
    assert_eq!(claim.monitor.state, OperationState::Unknown);
    assert!(claim.monitor.cancel_requested);
}

#[test]
fn plugin_monitor_deadline_fences_all_busy_workers_without_false_cancellation() {
    let ledger = ledger();
    let mut claims = Vec::new();
    for index in 0..4 {
        let mut registration = registration(&format!("busy-{index}"));
        registration.scope.plugin = format!("fixture-{index}");
        ledger.monitor_existing_job(registration, 100).unwrap();
        claims.push(ledger.claim_job_check(1100).unwrap().unwrap());
    }
    assert_eq!(ledger.next_job_check().unwrap(), Some(100_000));
    assert!(ledger.claim_job_check(100_000).unwrap().is_none());
    assert!(ledger.next_job_check().unwrap().is_none());
    for claim in claims {
        let registration = claim.monitor.registration.clone();
        assert!(ledger
            .settle_job_check(
                claim,
                &registration.job,
                OperationState::Succeeded,
                100_001,
                1000
            )
            .is_err());
        let record = ledger
            .job_monitor(&registration.operation)
            .unwrap()
            .unwrap();
        assert_eq!(record.state, OperationState::Unknown);
        assert_eq!(record.stopped, Some(MonitorStop::Deadline));
    }
}
