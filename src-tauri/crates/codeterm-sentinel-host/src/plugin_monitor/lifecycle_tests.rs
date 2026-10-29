use super::*;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use serde_json::json;

type ReconcileGate = (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
struct Adapter {
    submits: AtomicUsize,
    checks: AtomicUsize,
    cancels: AtomicUsize,
    gate: parking_lot::Mutex<Option<ReconcileGate>>,
}
impl BlueprintStep for Adapter {
    fn estimate(&self, _: &StepCall<'_>) -> Result<Option<Estimate>, String> {
        panic!("free fixture");
    }
    fn submit(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.submits.fetch_add(1, Ordering::SeqCst);
        Ok(JobUpdate {
            job: "existing".into(),
            state: OperationState::Queued,
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
            state: OperationState::Running,
            outcome: None,
            outputs: PortValues::new(),
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
    _root: tempfile::TempDir,
}
fn fixture(cancel: bool) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let (service, handle, _, context, _, clock) = super::tests::fixture(root.path());
    let descriptor = parse_descriptors(&json!([{"contractVersion":1,"revision":1,"id":"monitor-fixture.step","title":"Step","description":"No paid calls","params":{"type":"object","properties":{}},"inputs":{},"outputs":{},"outcomes":["done","failed"],"effects":{"sideEffects":false,"potentiallyPaid":false},"lifecycle":{"reconcile":true,"cancel":cancel}}])).unwrap().remove(0);
    let adapter = Arc::new(Adapter {
        submits: AtomicUsize::new(0),
        checks: AtomicUsize::new(0),
        cancels: AtomicUsize::new(0),
        gate: Default::default(),
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
    let owner = context.owner.unwrap();
    let workspace = context.workspace.unwrap();
    let mut job = handle.store.lock().prepare_document(json!({"schema_version":2,"kind":"pipeline","name":"Monitor fixture","owner":"tab","trigger":{"type":"input","kind":{"lit":"text"},"source":{"lit":{"text":"fixture"}}},
        "steps":[{"id":"step","uses":"monitor-fixture.step","plugin":{"revision":1,"spending":{"unit":"units","run_ceiling":0,"step_allowance":0}}}],"result":{"to":"notify"}}),crate::store::Owner {creator_tab_id:owner.clone(),workspace_id:workspace.clone(),team_id:None,cwd:root.path().to_string_lossy().into_owned()},false).unwrap();
    let scope = RunScope {
        workspace,
        team: None,
        run: "run".into(),
    };
    let request = StepRequest {
        context: StepContext {
            scope,
            owner,
            node: "step".into(),
            attempt: "attempt".into(),
            operation: "operation".into(),
            run_view: "run".into(),
            run_label: "Monitor run #1".into(),
            node_label: "Step".into(),
            input_lineage: vec![],
        },
        node_type: descriptor.id.clone(),
        revision: 1,
        params: json!({}),
        inputs: PortValues::new(),
        unit: "units".into(),
        step_allowance: 0,
    };
    let ledger = plugin::ledger(&service.service).unwrap();
    ledger
        .admit_run(&request.context.scope, "units", 0)
        .unwrap();
    let registered = service
        .service
        .ext::<Arc<GraphNodeRegistry>>()
        .step(&descriptor.id)
        .unwrap();
    StepRuntime {
        ledger: &ledger,
        descriptor: &descriptor,
        adapter: registered.adapter,
        media: None,
    }
    .drive(
        "monitor-fixture",
        &request,
        || 1000,
        Instant::now() + Duration::from_secs(2),
        Default::default(),
    )
    .unwrap();
    adapter.submits.store(0, Ordering::SeqCst);
    job.admission.plugin_hold = Some(codeterm_sentinel::admission::PluginHold {
        run: "run".into(),
        node: "step".into(),
        operation: "operation".into(),
        state: "queued".into(),
        reason: "plugin_step_queued".into(),
    });
    job.admission.continuation = Some(json!({"priorOutputs":{"kept":"original"}}));
    let job = storage::commit(&handle.store, |store| store.stage_insert(job))
        .unwrap()
        .id;
    prepare_monitors(&service.service, &handle).unwrap();
    Fixture {
        service,
        handle,
        adapter,
        request,
        descriptor,
        job,
        clock,
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
            Some(json!({"priorOutputs":{"kept":"original"}}))
        );
        assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 0);
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
    let service = fixture.service.service.weak_self().upgrade().unwrap();
    let handle = fixture.handle.clone();
    let worker = std::thread::spawn(move || run_check(&service, &handle, claim));
    let parked = observed.recv_timeout(Duration::from_secs(2));
    stop(&fixture);
    prepare_monitors(&fixture.service.service, &fixture.handle).unwrap();
    let intent = ledger.get("operation").unwrap().unwrap();
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
    assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 0);
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
        Some(json!({"priorOutputs":{"kept":"original"}}))
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
            .get("operation")
            .unwrap()
            .unwrap();
        assert_eq!(record.state, OperationState::Unknown);
        assert!(record.cancel_requested);
        assert_eq!(fixture.adapter.cancels.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.adapter.submits.load(Ordering::SeqCst), 0);
    }
}
