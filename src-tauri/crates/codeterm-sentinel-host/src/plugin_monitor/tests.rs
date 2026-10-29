use super::*;
use codeterm_plugin::{
    plugin_jobs::{JobCall, JobLifecycleDescriptor, JobObservation, PluginJobAdmission},
    view_host::{
        with_view_call_context, ViewCallContext, ViewError, ViewProvider, ViewRegistryHandle,
    },
};
use codeterm_service_core::{
    multiplexer::{
        state::MultiplexerState,
        types::{SurfaceBacking, TabBacking},
    },
    service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};

struct QueuedStep {
    submits: AtomicUsize,
    checks: AtomicUsize,
}
impl BlueprintStep for QueuedStep {
    fn estimate(&self, _: &StepCall<'_>) -> Result<Option<Estimate>, String> {
        panic!("free step must not estimate");
    }
    fn submit(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.submits.fetch_add(1, Ordering::SeqCst);
        Ok(JobUpdate {
            job: "existing-step-job".into(),
            state: OperationState::Queued,
            outcome: None,
            outputs: PortValues::new(),
        })
    }
    fn reconcile(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        assert_eq!(call.job, Some("existing-step-job"));
        self.checks.fetch_add(1, Ordering::SeqCst);
        Ok(JobUpdate {
            job: "existing-step-job".into(),
            state: OperationState::Succeeded,
            outcome: Some("done".into()),
            outputs: PortValues::new(),
        })
    }
    fn cancel(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        panic!("no cancellation requested");
    }
}

#[test]
fn plugin_monitor_blueprint_callback_reconciles_known_job_without_resubmission() {
    let root = tempfile::tempdir().unwrap();
    let (fixture, handle, _, context, _, clock) = fixture(root.path());
    let scope = RunScope {
        workspace: context.workspace.clone().unwrap(),
        team: context.team.clone(),
        run: "run".into(),
    };
    let descriptor = parse_descriptors(&json!([{"contractVersion":1,"revision":1,"id":"monitor-fixture.step","title":"Step","description":"No paid effects",
        "params":{"type":"object","properties":{}},"inputs":{},"outputs":{},"outcomes":["done","failed"],"effects":{"sideEffects":false,"potentiallyPaid":false},"lifecycle":{"reconcile":true,"cancel":false}}])).unwrap();
    let adapter = Arc::new(QueuedStep {
        submits: AtomicUsize::new(0),
        checks: AtomicUsize::new(0),
    });
    let steps = fixture.service.ext::<Arc<GraphNodeRegistry>>();
    steps
        .register_steps("monitor-fixture", &descriptor, adapter.clone())
        .unwrap();
    let registered = steps.step("monitor-fixture.step").unwrap();
    let ledger = plugin::ledger(&fixture.service).unwrap();
    ledger.admit_run(&scope, "units", 0).unwrap();
    let request = StepRequest {
        context: StepContext {
            scope,
            owner: context.owner.clone().unwrap(),
            node: "step".into(),
            attempt: "attempt".into(),
            operation: "operation".into(),
            run_view: "run".into(),
            run_label: "Fixture run #1".into(),
            node_label: "Step".into(),
            input_lineage: vec![],
        },
        node_type: "monitor-fixture.step".into(),
        revision: 1,
        params: json!({}),
        inputs: PortValues::new(),
        unit: "units".into(),
        step_allowance: 0,
    };
    let queued = StepRuntime {
        ledger: &ledger,
        descriptor: &descriptor[0],
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
    assert_eq!(queued.state, OperationState::Queued);
    ledger
        .monitor_existing_job(
            PluginJobRegistration {
                scope: PluginJobScope {
                    plugin: "monitor-fixture".into(),
                    workspace: request.context.scope.workspace.clone(),
                    team: context.team,
                    owner: context.owner.unwrap(),
                },
                origin: PluginJobOrigin::Blueprint {
                    blueprint: "fixture-blueprint".into(),
                    run: "run".into(),
                    node: "step".into(),
                    attempt: "attempt".into(),
                },
                operation: "operation".into(),
                job: "existing-step-job".into(),
                next_check_ms: 2000,
                deadline_ms: 10000,
            },
            1000,
        )
        .unwrap();
    clock.set_ms(2000);
    check(
        &fixture.service,
        &handle,
        ledger.claim_job_check(2000).unwrap().unwrap(),
    )
    .unwrap();
    assert_eq!(adapter.submits.load(Ordering::SeqCst), 1);
    assert_eq!(adapter.checks.load(Ordering::SeqCst), 1);
    assert_eq!(
        ledger.get("operation").unwrap().unwrap().state,
        OperationState::Succeeded
    );
}

struct Provider {
    checks: AtomicUsize,
    cancels: AtomicUsize,
}
#[codeterm_plugin::async_trait]
impl ViewProvider for Provider {
    fn id(&self) -> &str {
        "monitor-fixture"
    }
    fn job_lifecycle(&self) -> Option<JobLifecycleDescriptor> {
        Some(JobLifecycleDescriptor {
            contract_version: 1,
            reconcile: true,
            cancel: false,
        })
    }
    fn reconcile_job(&self, call: &JobCall) -> Result<JobObservation, ViewError> {
        assert!(!codeterm_plugin::view_host::has_view_call_context());
        assert!(codeterm_plugin::surface_invocation::current(self.id()).is_none());
        assert_eq!(call.context.job, "known-job");
        assert!(matches!(
            call.context.origin,
            PluginJobOrigin::StudioView { .. }
        ));
        let check = self.checks.fetch_add(1, Ordering::SeqCst);
        Ok(JobObservation {
            job: call.context.job.clone(),
            state: if check == 0 {
                OperationState::Running
            } else {
                OperationState::Succeeded
            },
        })
    }
    fn cancel_job(&self, _: &JobCall) -> Result<JobObservation, ViewError> {
        self.cancels.fetch_add(1, Ordering::SeqCst);
        panic!("unsupported cancellation must not dispatch");
    }
    async fn call(&self, _: &str, _: Value, _: &ViewCallContext) -> Result<Value, ViewError> {
        panic!("monitor must not enter a document export");
    }
}

fn fixture(
    root: &std::path::Path,
) -> (
    ServiceFixture,
    Handle,
    Arc<Provider>,
    ViewCallContext,
    String,
    Arc<codeterm_cron::clock::MockClock>,
) {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let tab = mux.create_tab(&workspace).unwrap();
    let view = mux.get_tab_mut(&tab).unwrap();
    view.parent_tab_id = Some(owner);
    view.backing = TabBacking::Surface(SurfaceBacking {
        plugin_id: "monitor-fixture".into(),
        surface_id: "studio".into(),
        instance_id: "instance".into(),
        owner_label: true,
        resume_url: None,
        resume_profile: None,
    });
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>()
        .register(
            "monitor-fixture",
            Arc::new(vec![serde_json::from_value(
                json!({"id":"studio","title":"Studio","renderer":"view","stateInvalidation":true}),
            )
            .unwrap()]),
        );
    assert!(fixture
        .service
        .register_host_extension(Arc::new(plugin::PluginStepStoreRoot(
            root.join("plugin-state")
        )))
        .is_ok());
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let store = crate::store::Store::new(root.join("sentinels.json"), clock.clone());
    let handle = crate::scheduler::fixture_handle_without_app(store);
    assert!(fixture
        .service
        .register_host_extension(Arc::new(handle.clone()))
        .is_ok());
    let provider = Arc::new(Provider {
        checks: AtomicUsize::new(0),
        cancels: AtomicUsize::new(0),
    });
    let registry = fixture.service.ext::<ViewRegistryHandle>();
    registry.register("monitor-fixture", provider.clone());
    let context = fixture
        .service
        .plugin_view_context(
            "monitor-fixture",
            Some("studio"),
            Some("instance"),
            Some(&tab),
        )
        .unwrap();
    let erased: Arc<dyn ViewProvider> = provider.clone();
    let frame = codeterm_plugin::media_delivery::create_frame(&erased, registry, context).unwrap();
    let context =
        codeterm_plugin::media_delivery::frame_context("monitor-fixture", &frame.media_frame)
            .unwrap();
    (fixture, handle, provider, context, frame.view_lease, clock)
}

fn admit(service: &ServiceState, context: &ViewCallContext) -> String {
    let admission = crate::plugin_job_admission::JobAdmission(Arc::downgrade(service));
    let receipt = with_view_call_context("monitor-fixture", context, || {
        admission
            .invoke("monitor-fixture", r#"{"action":"admit"}"#)
            .map_err(ViewError::Call)
    })
    .unwrap();
    receipt["operation"].as_str().unwrap().into()
}

#[test]
fn plugin_monitor_host_retains_receipt_after_frame_retirement_and_checks_exact_job_without_view_authority(
) {
    let root = tempfile::tempdir().unwrap();
    let (fixture, handle, provider, context, lease, clock) = fixture(root.path());
    let operation = admit(&fixture.service, &context);
    codeterm_plugin::media_delivery::revoke_view_frame("monitor-fixture", &lease);
    let admission = crate::plugin_job_admission::JobAdmission(Arc::downgrade(&fixture.service));
    let tracked = admission
        .invoke(
            "monitor-fixture",
            &json!({"action":"track","operation":operation,"job":"known-job"}).to_string(),
        )
        .unwrap();
    assert_eq!(tracked["registered"], true);
    assert!(tracked["monitor"].get("claim").is_none());
    let ledger = plugin::ledger(&fixture.service).unwrap();
    clock.set_ms(2000);
    let first = ledger.claim_job_check(2000).unwrap().unwrap();
    check(&fixture.service, &handle, first).unwrap();
    assert_eq!(
        ledger.job_monitor(&operation).unwrap().unwrap().state,
        OperationState::Running
    );
    clock.set_ms(7000);
    let second = ledger.claim_job_check(7000).unwrap().unwrap();
    check(&fixture.service, &handle, second).unwrap();
    assert_eq!(
        ledger.job_monitor(&operation).unwrap().unwrap().state,
        OperationState::Succeeded
    );
    assert_eq!(
        ledger
            .job_origin("monitor-fixture", &operation)
            .unwrap()
            .job
            .as_deref(),
        Some("known-job")
    );
    assert_eq!(provider.checks.load(Ordering::SeqCst), 2);
    assert_eq!(provider.cancels.load(Ordering::SeqCst), 0);
    assert!(admission
        .invoke(
            "monitor-fixture",
            &json!({"action":"track","operation":operation,"job":"replacement"}).to_string()
        )
        .is_err());
}

#[test]
fn plugin_monitor_host_refuses_replaced_document_scope_and_never_fabricates_unsupported_cancel() {
    let root = tempfile::tempdir().unwrap();
    let (fixture, handle, provider, context, _, clock) = fixture(root.path());
    let operation = admit(&fixture.service, &context);
    let admission = crate::plugin_job_admission::JobAdmission(Arc::downgrade(&fixture.service));
    admission
        .invoke(
            "monitor-fixture",
            &json!({"action":"track","operation":operation,"job":"known-job"}).to_string(),
        )
        .unwrap();
    let cancelled = with_view_call_context("monitor-fixture", &context, || {
        admission
            .invoke(
                "monitor-fixture",
                &json!({"action":"cancel","operation":operation}).to_string(),
            )
            .map_err(ViewError::Call)
    })
    .unwrap();
    assert_eq!(cancelled["monitor"]["state"], "unknown");
    let ledger = plugin::ledger(&fixture.service).unwrap();
    clock.set_ms(2000);
    let claim = ledger.claim_job_check(2000).unwrap().unwrap();
    assert!(claim.cancel);
    check(&fixture.service, &handle, claim).unwrap();
    assert_eq!(
        ledger.job_monitor(&operation).unwrap().unwrap().state,
        OperationState::Unknown
    );
    assert_eq!(provider.cancels.load(Ordering::SeqCst), 0);
    assert_eq!(provider.checks.load(Ordering::SeqCst), 0);
    fixture.service.ext::<ViewRegistryHandle>().register(
        "monitor-fixture",
        Arc::new(Provider {
            checks: AtomicUsize::new(0),
            cancels: AtomicUsize::new(0),
        }),
    );
    assert!(
        with_view_call_context("monitor-fixture", &context, || admission
            .invoke("monitor-fixture", r#"{"action":"admit"}"#)
            .map_err(ViewError::Call))
        .is_err()
    );
    assert!(admission
        .invoke(
            "monitor-fixture",
            &json!({"action":"admit","owner":"forged"}).to_string()
        )
        .is_err());
    assert_eq!(
        ledger
            .job_origin("monitor-fixture", &operation)
            .unwrap()
            .job
            .as_deref(),
        Some("known-job")
    );
}
