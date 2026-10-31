use super::*;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

struct ControlledSubmit {
    started: SyncSender<()>,
    release: Mutex<Receiver<()>>,
    submitted: AtomicUsize,
    canceled: AtomicUsize,
}

fn controlled() -> (Arc<ControlledSubmit>, Receiver<()>, SyncSender<()>) {
    let (started, entered) = sync_channel(0);
    let (resume, release) = sync_channel(0);
    (
        Arc::new(ControlledSubmit {
            started,
            release: Mutex::new(release),
            submitted: AtomicUsize::new(0),
            canceled: AtomicUsize::new(0),
        }),
        entered,
        resume,
    )
}

impl BlueprintStep for ControlledSubmit {
    fn submit(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.started
            .send(())
            .map_err(|_| "fixture receiver closed")?;
        self.release
            .lock()
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "fixture release missing")?;
        self.submitted.fetch_add(1, Ordering::SeqCst);
        Ok(result(OperationState::Queued))
    }
    fn reconcile(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        Ok(result(OperationState::Succeeded))
    }
    fn cancel(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.canceled.fetch_add(1, Ordering::SeqCst);
        assert_eq!(call.job, Some("job-1"));
        Ok(result(OperationState::Canceled))
    }
}

pub(super) fn reserve_pre_submit_cancel() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("operations.sqlite");
    let request = request("reserve-race");
    let owner = OperationLedger::open(&path).unwrap();
    let contender = OperationLedger::open(&path).unwrap();
    let descriptor = descriptor();
    let (fixture, entered, resume) = controlled();
    let runtime = runtime(&owner, &descriptor, fixture.clone());
    let competing = super::runtime(&contender, &descriptor, fixture.clone());
    std::thread::scope(|scope| {
        let submit = scope.spawn(|| drive(&runtime, &request));
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let cancellation = crate::converter::ConversionCancellation::default();
        cancellation.cancel();
        let losing = competing
            .drive(
                "fixture",
                &request,
                Instant::now() + Duration::from_secs(5),
                cancellation,
            )
            .unwrap();
        assert_eq!(losing.state, OperationState::Submitting);
        assert!(!losing.cancel_requested);
        resume.send(()).unwrap();
        let submitted = submit.join().unwrap();
        assert_eq!(submitted.state, OperationState::Queued);
        assert_eq!(submitted.job.as_deref(), Some("job-1"));
    });
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.canceled.load(Ordering::SeqCst), 0);
}

#[test]
fn plugin_steps_cancel_during_submit_retains_intent_and_eventual_job() {
    let request = request("cancel-race");
    let ledger = ledger();
    let descriptor = descriptor();
    let (fixture, entered, resume) = controlled();
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    std::thread::scope(|scope| {
        let submit = scope.spawn(|| drive(&runtime, &request));
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let held = runtime
            .cancel(&request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(held.state, OperationState::Unknown);
        assert!(held.cancel_requested);
        assert_eq!(held.job, None);
        assert!(runtime
            .reconcile(
                &request,
                Instant::now() + Duration::from_secs(5),
                Default::default()
            )
            .is_err());
        assert_eq!(fixture.canceled.load(Ordering::SeqCst), 0);
        resume.send(()).unwrap();
        let submitted = submit.join().unwrap();
        assert_eq!(submitted.state, OperationState::Unknown);
        assert_eq!(submitted.job.as_deref(), Some("job-1"));
    });
    let confirmed = runtime
        .cancel(&request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert_eq!(confirmed.state, OperationState::Canceled);
    assert_eq!(confirmed.job.as_deref(), Some("job-1"));
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.canceled.load(Ordering::SeqCst), 1);
}

struct MissingMedia;
impl StepMedia for MissingMedia {
    fn input(
        &self,
        _: &StepContext,
        _: &str,
        _: &str,
        _: PortKind,
    ) -> Result<InputLineage, String> {
        Err("fixture source missing".into())
    }
    fn publish(
        &self,
        _: &StepContext,
        _: &str,
        _: &str,
        _: &str,
        _: PortKind,
    ) -> Result<String, String> {
        Err("fixture media unavailable".into())
    }
}

#[test]
fn plugin_steps_invalid_output_outcome_or_media_preserves_job_across_restart() {
    for defect in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("operations.sqlite");
        let request = request("malformed");
        let mut descriptor = descriptor();
        let fixture = Fixture::new();
        let mut update = result(OperationState::Succeeded);
        update.job = "fixture-123".into();
        match defect {
            0 => {
                update.outputs =
                    [("result".into(), vec![PortValue::Json { value: json!({}) }])].into()
            }
            1 => update.outcome = Some("undeclared".into()),
            _ => {
                descriptor.outputs.get_mut("result").unwrap().kind = PortKind::Image;
                update.outputs = [(
                    "result".into(),
                    vec![PortValue::MediaSource {
                        id: "missing".into(),
                        media: PortKind::Image,
                    }],
                )]
                .into();
            }
        }
        *fixture.submit_result.lock() = Ok(update);
        {
            let ledger = OperationLedger::open(&path).unwrap();
            let runtime = StepRuntime {
                ledger: &ledger,
                descriptor: &descriptor,
                adapter: fixture.clone(),
                media: Some(&MissingMedia),
            };
            let unknown = drive(&runtime, &request);
            assert_eq!(unknown.state, OperationState::Unknown);
            assert_eq!(unknown.job.as_deref(), Some("fixture-123"));
        }
        let ledger = OperationLedger::open(&path).unwrap();
        let runtime = StepRuntime {
            ledger: &ledger,
            descriptor: &descriptor,
            adapter: fixture.clone(),
            media: Some(&MissingMedia),
        };
        assert_eq!(
            drive(&runtime, &request).job.as_deref(),
            Some("fixture-123")
        );
        let mut known = result(OperationState::Queued);
        known.job = "fixture-123".into();
        *fixture.status_result.lock() = Ok(known);
        let reconciled = runtime
            .reconcile(
                &request,
                Instant::now() + Duration::from_secs(5),
                Default::default(),
            )
            .unwrap();
        assert_eq!(reconciled.state, OperationState::Queued);
        assert_eq!(
            fixture.reconciled_jobs.lock().last().cloned(),
            Some(Some("fixture-123".into()))
        );
        assert_eq!(fixture.submitted.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn plugin_steps_revoked_inflight_adapter_preserves_returned_job_as_unknown() {
    let request = request("revocation");
    let ledger = ledger();
    let descriptor = descriptor();
    let registry = GraphNodeRegistry::default();
    let (fixture, entered, resume) = controlled();
    registry
        .register_steps("fixture", &[descriptor.clone()], fixture.clone())
        .unwrap();
    let registered = registry.step("fixture.generate").unwrap();
    let runtime = runtime(&ledger, &registered.descriptor, registered.adapter.clone());
    std::thread::scope(|scope| {
        let submit = scope.spawn(|| drive(&runtime, &request));
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        registry.unregister("fixture");
        resume.send(()).unwrap();
        let unknown = submit.join().unwrap();
        assert_eq!(unknown.state, OperationState::Unknown);
        assert_eq!(unknown.job.as_deref(), Some("job-1"));
    });
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 1);
}

#[test]
fn plugin_steps_malformed_js_payload_retains_job_for_reconciliation_without_replay() {
    let vm = Arc::new(crate::js_runtime::PluginVm::from_source("blueprint-malformed", r#"let submitted=0; module.exports={
        blueprintSubmit:()=>{submitted++;return {job:'fixture-js',state:'not-a-state',outputs:'bad'};},
        blueprintReconcile:(r)=>{if(r.job!=='fixture-js')throw Error('missing job');return {job:r.job,state:'queued',outcome:null,outputs:{}};},
        submissionCount:()=>submitted
    };"#.into()).unwrap());
    let adapter = Arc::new(crate::js_runtime::JsBlueprintStep::new(vm.clone()));
    let request = request("js-invalid");
    let descriptor = descriptor();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("operations.sqlite");
    {
        let ledger = OperationLedger::open(&path).unwrap();
        let runtime = runtime(&ledger, &descriptor, adapter.clone());
        let unknown = drive(&runtime, &request);
        assert_eq!(unknown.state, OperationState::Unknown);
        assert_eq!(unknown.job.as_deref(), Some("fixture-js"));
    }
    let ledger = OperationLedger::open(&path).unwrap();
    let runtime = runtime(&ledger, &descriptor, adapter);
    assert_eq!(drive(&runtime, &request).state, OperationState::Unknown);
    assert_eq!(
        runtime
            .reconcile(
                &request,
                Instant::now() + Duration::from_secs(5),
                Default::default()
            )
            .unwrap()
            .state,
        OperationState::Queued
    );
    assert_eq!(
        crate::js_runtime::call_json::<u32>(&vm, "submissionCount", &[]).unwrap(),
        1
    );
}
