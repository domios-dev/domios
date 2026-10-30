use super::*;
use crate::blueprint_pipeline::BlueprintExt;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use serde_json::json;

type ReconcileGate = (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
struct Adapter {
    submits: AtomicUsize,
    checks: AtomicUsize,
    cancels: AtomicUsize,
    initial: OperationState,
    gate: parking_lot::Mutex<Option<ReconcileGate>>,
    complete: std::sync::atomic::AtomicBool,
}
impl BlueprintStep for Adapter {
    fn submit(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.submits.fetch_add(1, Ordering::SeqCst);
        Ok(JobUpdate {
            job: "existing".into(),
            state: self.initial,
            outcome: None,
            outputs: PortValues::new(),
        })
    }
    fn reconcile(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        assert_eq!(call.job, Some("existing"));
        self.checks.fetch_add(1, Ordering::SeqCst);
        if let Some((entered, release)) = self.gate.lock().take() {
            entered.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        Ok(JobUpdate {
            job: "existing".into(),
            state: if self.complete.load(Ordering::SeqCst) {
                OperationState::Succeeded
            } else {
                OperationState::Running
            },
            outcome: self.complete.load(Ordering::SeqCst).then(|| "done".into()),
            outputs: if self.complete.load(Ordering::SeqCst) {
                [("text".into(), vec![PortValue::Text { text: "opaque final".into() }])].into()
            } else {
                PortValues::new()
            },
        })
    }
    fn cancel(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        assert_eq!(call.job, Some("existing"));
        self.cancels.fetch_add(1, Ordering::SeqCst);
        Ok(JobUpdate::unknown("existing".into()))
    }
}

struct Fixture {
    service: ServiceFixture,
    handle: Handle,
    adapter: Arc<Adapter>,
    request: StepRequest,
    descriptor: StepDescriptor,
    job: String,
    clock: Arc<codeterm_cron::clock::MockClock>,
    continuation: serde_json::Value,
    storage: codeterm_storage::Storage,
    _root: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.storage
            .shutdown()
            .wait(Duration::from_secs(10))
            .unwrap();
    }
}

fn fixture(cancel: bool) -> Fixture {
    fixture_with_state(cancel, OperationState::Queued)
}

fn fixture_with_state(cancel: bool, initial: OperationState) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let (mux, owner) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let service = ServiceFixture::new(mux);
    assert!(service
        .service
        .register_host_extension(Arc::new(plugin::PluginStepStoreRoot(
            root.path().join("plugin-state")
        )))
        .is_ok());
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    codeterm_storage::test_support::seed_empty_root(&root.path().join("storage"));
    let storage =
        codeterm_storage::Storage::open(&root.path().join("storage"), Default::default()).unwrap();
    let handle = crate::scheduler::fixture_handle_without_app(crate::store::Store::with_storage(
        &storage,
        clock.clone(),
    ));
    let descriptor = parse_descriptors(&json!([{"contractVersion":1,"revision":1,"id":"monitor-fixture.step","title":"Step","description":"Lifecycle fixture","params":{"type":"object","properties":{}},"inputs":{},"outputs":{"text":{"kind":"text","minItems":1,"maxItems":1}},"outcomes":["done","failed"],"effects":{"sideEffects":false},"lifecycle":{"reconcile":true,"cancel":cancel}}])).unwrap().remove(0);
    let adapter = Arc::new(Adapter {
        submits: AtomicUsize::new(0),
        checks: AtomicUsize::new(0),
        cancels: AtomicUsize::new(0),
        initial,
        gate: Default::default(),
        complete: std::sync::atomic::AtomicBool::new(false),
    });
    service
        .service
        .ext::<Arc<GraphNodeRegistry>>()
        .register_steps(
            "monitor-fixture",
            std::slice::from_ref(&descriptor),
            adapter.clone(),
        )
        .unwrap();
    let mut job = handle.store.lock().prepare_document(json!({"schema_version":2,"kind":"pipeline","name":"Monitor fixture","owner":"tab","trigger":{"type":"input","kind":{"lit":"text"},"source":{"lit":{"text":"fixture"}}},
        "steps":[{"id":"step","uses":"monitor-fixture.step","limits":{"wall_ms":10000}},{"id":"downstream","uses":"filter","with":{"pattern":{"lit":".*"}}}],"result":{"to":"notify"}}),crate::store::Owner {creator_tab_id:owner.clone(),workspace_id:workspace.clone(),team_id:None,cwd:root.path().to_string_lossy().into_owned()},false).unwrap();
    let caller = crate::service_adapter::SentinelCaller {
        tab: owner,
        team: None,
        workspace,
        cwd: root.path().to_string_lossy().into_owned(),
        nonce: None,
    };
    let preview = service
        .service
        .blueprint_pipeline_validate(&caller, job.pipeline().unwrap().clone())
        .unwrap();
    assert!(
        preview.checks.iter().all(|check| check.passed),
        "{:?}",
        preview.checks
    );
    job.set_pipeline(Some(preview.normalized_spec));
    let persisted = storage::commit(&handle.store, |store| store.stage_insert(job)).unwrap();
    let reopened = crate::store::Store::with_storage(&storage, clock.clone());
    assert_eq!(
        reopened.get(&persisted.id).unwrap().pipeline(),
        persisted.pipeline()
    );
    let lease = crate::store::Lease {
        job: persisted,
        cancelled: Default::default(),
        event: None,
    };
    assert_eq!(
        crate::pipeline::run_plugin_fixture(&handle, &service.service, &lease, "run").unwrap(),
        codeterm_blueprint_types::blueprint_journal::JournalState::Held
    );
    let request = plugin::ledger(&service.service)
        .unwrap()
        .get(&plugin::operation("run", "step"))
        .unwrap()
        .unwrap()
        .request;
    assert!(crate::pipeline::write_journal(&handle, &lease.job,
        codeterm_storage::BlueprintJournalWrite::CloseRun {
            run_id: "run".into(),
            state: codeterm_blueprint_types::blueprint_journal::JournalState::Held,
            outcome: "held".into(),
            at_ms: 1000,
            reason: Some("plugin_step_running".into()),
        }));
    let job = lease.job.id;
    let continuation = handle
        .store
        .lock()
        .get(&job)
        .unwrap()
        .admission
        .continuation
        .clone()
        .unwrap();
    prepare_monitors(&service.service, &handle).unwrap();
    Fixture {
        service,
        handle,
        adapter,
        request,
        descriptor,
        job,
        clock,
        continuation,
        storage,
        _root: root,
    }
}

fn stop(fixture: &Fixture) {
    let mut job = fixture
        .handle
        .store
        .lock()
        .get(&fixture.job)
        .unwrap()
        .clone();
    job.machine.stopped = true;
    storage::commit(&fixture.handle.store, |store| store.stage_finish(&job)).unwrap();
}

#[test]
fn plugin_monitor_updates_exact_queued_run_to_running_and_refuses_removed_or_revised_adapter() {
    for removed in [false, true] {
        let fixture = fixture(true);
        let ledger = plugin::ledger(&fixture.service.service).unwrap();
        fixture.clock.set_ms(2000);
        run_check(
            &fixture.service.service,
            &fixture.handle,
            ledger.claim_job_check(2000).unwrap().unwrap(),
        )
        .unwrap();
        let held = fixture
            .handle
            .store
            .lock()
            .get(&fixture.job)
            .unwrap()
            .admission
            .plugin_hold
            .clone()
            .unwrap();
        assert_eq!(held.state, "running");
        assert_eq!(held.reason, "plugin_step_running");
        let steps = fixture.service.service.ext::<Arc<GraphNodeRegistry>>();
        if removed {
            steps.unregister("monitor-fixture");
        } else {
            let mut revised = fixture.descriptor.clone();
            revised.revision = 2;
            steps
                .register_steps("monitor-fixture", &[revised], fixture.adapter.clone())
                .unwrap();
        }
        fixture.clock.set_ms(7000);
        assert!(run_check(
            &fixture.service.service,
            &fixture.handle,
            ledger.claim_job_check(7000).unwrap().unwrap()
        )
        .is_err());
        let job = fixture
            .handle
            .store
            .lock()
            .get(&fixture.job)
            .unwrap()
            .clone();
        assert_eq!(job.admission.plugin_hold.as_ref().unwrap().state, "unknown");
        assert_eq!(
            job.admission.plugin_hold.as_ref().unwrap().reason,
            "plugin_step_unavailable"
        );
        assert_eq!(
            job.admission.continuation,
            Some(fixture.continuation.clone())
        );
        assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn plugin_monitor_stop_during_reconcile_cancels_exact_job_once_after_restart_without_false_terminal_or_output_loss(
) {
    let fixture = fixture(true);
    let ledger = plugin::ledger(&fixture.service.service).unwrap();
    let (entered, observed) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    *fixture.adapter.gate.lock() = Some((entered, resume));
    fixture.clock.set_ms(2000);
    let claim = ledger.claim_job_check(2000).unwrap().unwrap();
    let service = fixture
        .service
        .service
        .ext::<std::sync::Weak<codeterm_service_core::service::ServiceContext>>()
        .clone()
        .upgrade()
        .unwrap();
    let handle = fixture.handle.clone();
    let worker = std::thread::spawn(move || run_check(&service, &handle, claim));
    let parked = observed.recv_timeout(Duration::from_secs(2));
    stop(&fixture);
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    let intent = ledger
        .get(&fixture.request.context.operation)
        .unwrap()
        .unwrap();
    assert!(intent.cancel_requested);
    assert!(!intent.cancel_dispatched);
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    assert!(parked.is_ok());
    fixture.clock.set_ms(3000);
    run_check(
        &fixture.service.service,
        &fixture.handle,
        ledger.claim_job_check(3000).unwrap().unwrap(),
    )
    .unwrap();
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    assert_eq!(fixture.adapter.cancels.load(Ordering::SeqCst), 1);
    drop(ledger);
    let reopened = plugin::ledger(&fixture.service.service).unwrap();
    reopened.recover_inflight().unwrap();
    let registered = fixture
        .service
        .service
        .ext::<Arc<GraphNodeRegistry>>()
        .step(&fixture.descriptor.id)
        .unwrap();
    let record = StepRuntime {
        ledger: &reopened,
        descriptor: &fixture.descriptor,
        adapter: registered.adapter,
        media: None,
    }
    .cancel(&fixture.request, Instant::now() + Duration::from_secs(2))
    .unwrap();
    assert_eq!(record.state, OperationState::Unknown);
    assert!(record.cancel_dispatched);
    assert_eq!(fixture.adapter.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 1);
    let job = fixture
        .handle
        .store
        .lock()
        .get(&fixture.job)
        .unwrap()
        .clone();
    assert!(job.machine.stopped);
    assert_eq!(
        job.admission.continuation,
        Some(fixture.continuation.clone())
    );
}

#[test]
fn plugin_monitor_scope_loss_and_unsupported_cancel_preserve_unknown_without_new_authority() {
    for scope_lost in [false, true] {
        let fixture = fixture(scope_lost);
        if scope_lost {
            fixture
                .service
                .service
                .mux
                .write()
                .get_tab_mut(&fixture.request.context.owner)
                .unwrap()
                .team_id = Some("other-team".into());
        }
        stop(&fixture);
        prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
        let record = plugin::ledger(&fixture.service.service)
            .unwrap()
            .get(&fixture.request.context.operation)
            .unwrap()
            .unwrap();
        assert_eq!(record.state, OperationState::Unknown);
        assert!(record.cancel_requested);
        assert_eq!(fixture.adapter.cancels.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn plugin_monitor_reconciles_running_job_and_resumes_run() {
    let fixture = fixture_with_state(false, OperationState::Running);
    let held = fixture
        .handle
        .store
        .lock()
        .get(&fixture.job)
        .unwrap()
        .admission
        .plugin_hold
        .clone()
        .unwrap();
    assert_eq!(held.reason, "plugin_step_running");
    fixture.adapter.complete.store(true, Ordering::SeqCst);
    fixture.clock.set_ms(2000);
    let ledger = plugin::ledger(&fixture.service.service).unwrap();
    let claim = ledger.claim_job_check(2000).unwrap().unwrap();
    run_check(&fixture.service.service, &fixture.handle, claim).unwrap();
    let job = fixture
        .handle
        .store
        .lock()
        .get(&fixture.job)
        .unwrap()
        .clone();
    assert!(job.admission.plugin_hold.is_none());
    assert!(job.machine.held.is_none());
    assert!(job.admission.continuation.is_some());
    let mut leases = storage::commit(&fixture.handle.store, |store| store.stage_due()).unwrap();
    assert_eq!(leases.len(), 1);
    let lease = leases.pop().unwrap();
    assert_eq!(
        crate::pipeline::run_plugin_fixture(
            &fixture.handle,
            &fixture.service.service,
            &lease,
            "run"
        )
        .unwrap(),
        codeterm_blueprint_types::blueprint_journal::JournalState::Done
    );
    fixture.handle.store.lock().release(&fixture.job);
    let record = ledger
        .get(&fixture.request.context.operation)
        .unwrap()
        .unwrap();
    assert_eq!(record.outcome.as_deref(), Some("done"));
    assert_eq!(record.outputs["text"], vec![PortValue::Text { text: "opaque final".into() }]);
    let read = fixture.handle.store.lock().typed_pipeline_log(&fixture.job, 1).unwrap();
    assert!(read.wait(storage::COMMIT_DEADLINE).unwrap().value.iter().any(|entry| entry.step_name == "downstream" && entry.state == codeterm_blueprint_types::blueprint_journal::JournalState::Done));
    assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.adapter.checks.load(Ordering::SeqCst), 1);
}

#[test]
fn plugin_monitor_completion_before_original_close_defers_resume_until_lease_release() {
    let fixture = fixture_with_state(false, OperationState::Running);
    fixture.handle.store.lock().active.insert(fixture.job.clone(), Default::default());
    fixture.adapter.complete.store(true, Ordering::SeqCst);
    fixture.clock.set_ms(2000);
    let ledger = plugin::ledger(&fixture.service.service).unwrap();
    run_check(&fixture.service.service, &fixture.handle, ledger.claim_job_check(2000).unwrap().unwrap()).unwrap();
    assert!(fixture.handle.store.lock().get(&fixture.job).unwrap().admission.plugin_hold.is_some());
    let job = fixture.handle.store.lock().get(&fixture.job).unwrap().clone();
    assert!(crate::pipeline::write_journal(&fixture.handle, &job,
        codeterm_storage::BlueprintJournalWrite::CloseRun {
            run_id: "run".into(), state: codeterm_blueprint_types::blueprint_journal::JournalState::Held,
            outcome: "held".into(), at_ms: 2000, reason: Some("plugin_step_running".into()),
        }));
    fixture.handle.store.lock().release(&fixture.job);
    fixture.handle.notify();
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    let job = fixture.handle.store.lock().get(&fixture.job).unwrap().clone();
    assert!(job.admission.plugin_hold.is_none());
    assert!(job.machine.held.is_none());
    assert_eq!(storage::commit(&fixture.handle.store, |store| store.stage_due()).unwrap().len(), 1);
    fixture.handle.store.lock().release(&fixture.job);
    assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.adapter.checks.load(Ordering::SeqCst), 1);
}

#[test]
fn plugin_monitor_authored_deadline_stops_unobserved_run_without_replay() {
    let fixture = fixture_with_state(false, OperationState::Running);
    let job = fixture.handle.store.lock().get(&fixture.job).unwrap().clone();
    let held = job.admission.plugin_hold.as_ref().unwrap();
    let deadline = step_deadline(&fixture.handle, &job, held).unwrap();
    assert_eq!(deadline, 11_000);
    fixture.clock.set_ms(deadline - 1);
    assert_eq!(prepare_monitors(&fixture.service.service, &fixture.handle).unwrap(), Some(deadline));
    assert!(!fixture.handle.store.lock().get(&fixture.job).unwrap().machine.stopped);
    fixture.clock.set_ms(deadline);
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    let job = fixture.handle.store.lock().get(&fixture.job).unwrap().clone();
    assert!(job.machine.stopped);
    assert_eq!(job.last_reason.as_deref(), Some("plugin_step_timeout"));
    assert!(storage::commit(&fixture.handle.store, |store| store.stage_due()).unwrap().is_empty());
    let read = fixture.handle.store.lock().typed_pipeline_log(&fixture.job, 1).unwrap();
    let entries = read.wait(storage::COMMIT_DEADLINE).unwrap().value;
    let close = entries.iter().find(|entry| entry.step_name == "$run").unwrap();
    assert_eq!(close.state, codeterm_blueprint_types::blueprint_journal::JournalState::Failed);
    assert_eq!(close.reason, Some(codeterm_blueprint_types::blueprint_journal::JournalReason::Deadline));
    assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.adapter.checks.load(Ordering::SeqCst), 0);
}

#[test]
fn plugin_monitor_completion_during_observation_releases_hold_once() {
    let fixture = fixture_with_state(false, OperationState::Running);
    let ledger = plugin::ledger(&fixture.service.service).unwrap();
    let (entered, observed) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    *fixture.adapter.gate.lock() = Some((entered, resume));
    fixture.clock.set_ms(2000);
    let claim = ledger.claim_job_check(2000).unwrap().unwrap();
    let service = fixture.service.service.ext::<std::sync::Weak<ServiceContext>>().upgrade().unwrap();
    let handle = fixture.handle.clone();
    let worker = std::thread::spawn(move || run_check(&service, &handle, claim));
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    fixture.adapter.complete.store(true, Ordering::SeqCst);
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    let job = fixture.handle.store.lock().get(&fixture.job).unwrap().clone();
    assert!(job.machine.held.is_none());
    assert!(job.admission.plugin_hold.is_none());
    assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.adapter.checks.load(Ordering::SeqCst), 1);
    assert!(ledger.claim_job_check(7000).unwrap().is_none());
}

#[test]
fn plugin_monitor_foreign_held_run_cannot_consume_completion() {
    let fixture = fixture_with_state(false, OperationState::Running);
    storage::commit(&fixture.handle.store, |store| {
        let mut job = store.get(&fixture.job).unwrap().clone();
        job.admission.plugin_hold.as_mut().unwrap().run = "foreign-run".into();
        store.stage_admission(&fixture.job, job.admission)
    }).unwrap();
    fixture.adapter.complete.store(true, Ordering::SeqCst);
    fixture.clock.set_ms(2000);
    let ledger = plugin::ledger(&fixture.service.service).unwrap();
    let claim = ledger.claim_job_check(2000).unwrap().unwrap();
    assert!(run_check(&fixture.service.service, &fixture.handle, claim).is_err());
    let job = fixture.handle.store.lock().get(&fixture.job).unwrap().clone();
    assert_eq!(job.admission.plugin_hold.as_ref().unwrap().run, "foreign-run");
    assert!(job.machine.held.is_some());
    assert_eq!(fixture.adapter.checks.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 1);
}

#[test]
fn plugin_monitor_deadline_during_observation_cannot_release_late_success() {
    let fixture = fixture_with_state(false, OperationState::Running);
    let ledger = plugin::ledger(&fixture.service.service).unwrap();
    let (entered, observed) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    *fixture.adapter.gate.lock() = Some((entered, resume));
    fixture.clock.set_ms(2000);
    let claim = ledger.claim_job_check(2000).unwrap().unwrap();
    let service = fixture.service.service.ext::<std::sync::Weak<ServiceContext>>().upgrade().unwrap();
    let handle = fixture.handle.clone();
    let worker = std::thread::spawn(move || run_check(&service, &handle, claim));
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    fixture.clock.set_ms(11_000);
    fixture.adapter.complete.store(true, Ordering::SeqCst);
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    let job = fixture.handle.store.lock().get(&fixture.job).unwrap().clone();
    assert!(job.machine.stopped);
    assert_eq!(job.last_reason.as_deref(), Some("plugin_step_timeout"));
    assert!(storage::commit(&fixture.handle.store, |store| store.stage_due()).unwrap().is_empty());
    assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.adapter.checks.load(Ordering::SeqCst), 1);
}

#[test]
fn plugin_monitor_deadline_dispatches_supported_cancel_once_before_monitor_stop() {
    let fixture = fixture_with_state(true, OperationState::Running);
    let ledger = plugin::ledger(&fixture.service.service).unwrap();
    fixture.clock.set_ms(11_000);
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    let record = ledger.get(&fixture.request.context.operation).unwrap().unwrap();
    assert!(record.cancel_requested);
    assert!(!record.cancel_dispatched);
    assert!(ledger.job_monitor(&record.request.context.operation).unwrap().unwrap().stopped.is_none());
    assert_eq!(fixture.adapter.cancels.load(Ordering::SeqCst), 0);
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    let claim = ledger.claim_job_check(11_000).unwrap().unwrap();
    run_check(&fixture.service.service, &fixture.handle, claim).unwrap();
    assert_eq!(fixture.adapter.cancels.load(Ordering::SeqCst), 1);
    assert!(ledger.get(&record.request.context.operation).unwrap().unwrap().cancel_dispatched);
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    assert!(ledger.job_monitor(&record.request.context.operation).unwrap().unwrap().stopped.is_some());
    assert!(ledger.claim_job_check(16_000).unwrap().is_none());
    assert_eq!(fixture.adapter.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 1);
    assert!(storage::commit(&fixture.handle.store, |store| store.stage_due()).unwrap().is_empty());
    let read = fixture.handle.store.lock().typed_pipeline_log(&fixture.job, 1).unwrap();
    let entries = read.wait(storage::COMMIT_DEADLINE).unwrap().value;
    let close = entries.iter().find(|entry| entry.step_name == "$run").unwrap();
    assert_eq!(close.state, JournalState::Failed);
    assert_eq!(close.reason, Some(JournalReason::Deadline));
}

#[test]
fn plugin_monitor_missing_claim_is_isolated_while_second_held_job_times_out() {
    let fixture = fixture_with_state(false, OperationState::Running);
    let original = fixture.handle.store.lock().get(&fixture.job).unwrap().clone();
    let mut document = original.document.clone().unwrap();
    document["name"] = json!("Healthy monitor");
    let mut healthy = fixture.handle.store.lock().prepare_document(document, crate::store::Owner {
        creator_tab_id: original.creator_tab_id.clone(), workspace_id: original.workspace_id.clone(),
        team_id: original.team_id.clone(), cwd: original.cwd.clone(),
    }, false).unwrap();
    let mut pipeline = original.pipeline().unwrap().clone();
    pipeline.name = "Healthy monitor".into();
    healthy.set_pipeline(Some(pipeline));
    let healthy = storage::commit(&fixture.handle.store, |store| store.stage_insert(healthy)).unwrap();
    let lease = crate::store::Lease { job: healthy, cancelled: Default::default(), event: None };
    assert_eq!(crate::pipeline::run_plugin_fixture(&fixture.handle, &fixture.service.service, &lease, "healthy-run").unwrap(), JournalState::Held);
    let ledger = plugin::ledger(&fixture.service.service).unwrap();
    let mut missing = fixture.request.clone();
    missing.context.operation = "missing-claim-operation".into();
    missing.context.scope.run = "missing-claim-run".into();
    missing.context.node = "missing-claim-node".into();
    ledger.prepare("monitor-fixture", &missing).unwrap();
    storage::commit(&fixture.handle.store, |store| {
        let mut job = store.get(&fixture.job).unwrap().clone();
        let held = job.admission.plugin_hold.as_mut().unwrap();
        held.operation = missing.context.operation.clone();
        held.run = missing.context.scope.run.clone();
        held.node = missing.context.node.clone();
        store.stage_admission(&fixture.job, job.admission)
    }).unwrap();
    fixture.clock.set_ms(11_000);
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    let bad = fixture.handle.store.lock().get(&fixture.job).unwrap().clone();
    assert!(!bad.machine.stopped);
    assert!(bad.admission.plugin_hold.is_some());
    let healthy = fixture.handle.store.lock().get(&lease.job.id).unwrap().clone();
    assert!(healthy.machine.stopped);
    assert_eq!(healthy.last_reason.as_deref(), Some("plugin_step_timeout"));
    assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.adapter.checks.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.adapter.cancels.load(Ordering::SeqCst), 0);
}

#[test]
fn plugin_monitor_stale_timeout_hold_cannot_mutate_ledger_or_monitor() {
    let fixture = fixture_with_state(true, OperationState::Running);
    let original = fixture.handle.store.lock().get(&fixture.job).unwrap().clone();
    let mut stale = original.admission.plugin_hold.clone().unwrap();
    stale.run = "foreign-run".into();
    let ledger = plugin::ledger(&fixture.service.service).unwrap();
    let before = ledger.get(&fixture.request.context.operation).unwrap().unwrap();
    let monitor = ledger.job_monitor(&fixture.request.context.operation).unwrap().unwrap();
    expire_step(&fixture.service.service, &fixture.handle, &original, &stale).unwrap();
    assert_eq!(ledger.get(&fixture.request.context.operation).unwrap().unwrap(), before);
    assert_eq!(ledger.job_monitor(&fixture.request.context.operation).unwrap().unwrap(), monitor);
    assert!(!fixture.handle.store.lock().get(&fixture.job).unwrap().machine.stopped);
}
