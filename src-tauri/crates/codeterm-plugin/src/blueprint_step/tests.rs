use super::*;
use crate::graph_nodes::GraphNodeRegistry;
use parking_lot::Mutex;
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

fn descriptor() -> StepDescriptor {
    parse_descriptors(&json!([{
        "contractVersion":1,"revision":1,"id":"fixture.generate","title":"Generate","description":"Fixture operation",
        "params":{"type":"object","properties":{"model":{"type":"string","enum":["small","large"],"maxLength":16}},"required":["model"]},
        "inputs":{"prompt":{"kind":"text","minItems":1,"maxItems":1}},
        "outputs":{"result":{"kind":"text","minItems":1,"maxItems":1}},
        "outcomes":["done","failed","pass","retry","choose"],"lifecycle":{"reconcile":true,"cancel":true},
        "effects":{"sideEffects":true,"potentiallyPaid":true}
    }])).unwrap().remove(0)
}

fn request(operation: &str) -> StepRequest {
    StepRequest {
        context: StepContext {
            scope: RunScope {
                workspace: "workspace".into(),
                team: Some("team".into()),
                run: "run".into(),
            },
            owner: "owner".into(),
            node: operation.into(),
            attempt: "attempt".into(),
            operation: operation.into(),
            run_view: "fixture-run-view".into(),
            run_label: "Fixture run #1".into(),
            node_label: "Generate".into(),
            input_lineage: vec![],
        },
        node_type: "fixture.generate".into(),
        revision: 1,
        params: json!({"model":"small"}),
        inputs: [(
            "prompt".into(),
            vec![PortValue::Text {
                text: "opaque fixture input".into(),
            }],
        )]
        .into(),
        unit: "credit_milli".into(),
        step_allowance: 10,
    }
}

fn result(state: OperationState) -> JobUpdate {
    JobUpdate {
        job: "job-1".into(),
        state,
        outcome: (state == OperationState::Succeeded).then(|| "done".into()),
        outputs: if state == OperationState::Succeeded {
            [(
                "result".into(),
                vec![PortValue::Text {
                    text: "fixture output".into(),
                }],
            )]
            .into()
        } else {
            Default::default()
        },
    }
}

struct Fixture {
    estimate: Mutex<Option<Estimate>>,
    submitted: AtomicUsize,
    submit_result: Mutex<Result<JobUpdate, String>>,
    status_result: Mutex<Result<JobUpdate, String>>,
    reconciled_jobs: Mutex<Vec<Option<String>>>,
}

impl Fixture {
    fn new(amount: u64) -> Arc<Self> {
        Arc::new(Self {
            estimate: Mutex::new(Some(Estimate {
                amount,
                unit: "credit_milli".into(),
                valid_until_ms: 200,
            })),
            submitted: AtomicUsize::new(0),
            submit_result: Mutex::new(Ok(result(OperationState::Queued))),
            status_result: Mutex::new(Ok(result(OperationState::Succeeded))),
            reconciled_jobs: Mutex::new(Vec::new()),
        })
    }
}

impl BlueprintStep for Fixture {
    fn estimate(&self, _: &StepCall<'_>) -> Result<Option<Estimate>, String> {
        Ok(self.estimate.lock().clone())
    }
    fn submit(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.submitted.fetch_add(1, Ordering::SeqCst);
        self.submit_result.lock().clone()
    }
    fn reconcile(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.reconciled_jobs
            .lock()
            .push(call.job.map(str::to_string));
        self.status_result.lock().clone()
    }
    fn cancel(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        Ok(result(OperationState::Canceled))
    }
}

fn ledger(request: &StepRequest, ceiling: u64) -> OperationLedger {
    let ledger =
        OperationLedger::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    ledger
        .admit_run(&request.context.scope, &request.unit, ceiling)
        .unwrap();
    ledger
}

fn runtime<'a>(
    ledger: &'a OperationLedger,
    descriptor: &'a StepDescriptor,
    fixture: Arc<dyn BlueprintStep>,
) -> StepRuntime<'a> {
    StepRuntime {
        ledger,
        descriptor,
        adapter: fixture,
        media: None,
    }
}

fn drive(runtime: &StepRuntime<'_>, request: &StepRequest) -> OperationRecord {
    runtime
        .drive(
            "fixture",
            request,
            || 100,
            Instant::now() + Duration::from_secs(5),
            Default::default(),
        )
        .unwrap()
}

#[test]
fn plugin_steps_descriptor_set_roundtrips_specialized_and_general_nodes() {
    let first = descriptor();
    let mut second = first.clone();
    second.id = "fixture.variation".into();
    second.inputs.insert(
        "references".into(),
        StepPort {
            kind: PortKind::Image,
            min_items: 0,
            max_items: 4,
            schema: None,
        },
    );
    let value = json!({"steps":[first,second]});
    let cap = crate::js_runtime::manifest::GraphNodesCap::from_value(&value).unwrap();
    assert!(cap.enabled());
    assert_eq!(cap.steps.len(), 2);
    assert_eq!(
        crate::js_runtime::manifest::GraphNodesCap::from_value(&cap.to_value()).unwrap(),
        cap
    );
    assert_eq!(cap.steps[1].inputs["references"].max_items, 4);
}

#[test]
fn plugin_steps_descriptor_refuses_open_schemas_collisions_and_versions() {
    let valid = serde_json::to_value(descriptor()).unwrap();
    for (path, value) in [
        ("contractVersion", json!(2)),
        ("revision", json!(0)),
        ("id", json!("unnamespaced")),
        ("outcomes", json!(["unknown"])),
    ] {
        let mut invalid = valid.clone();
        invalid[path] = value;
        assert!(parse_descriptors(&json!([invalid])).is_err());
    }
    let mut invalid = valid.clone();
    invalid["params"]["$ref"] = json!("remote");
    assert!(parse_descriptors(&json!([invalid])).is_err());
    assert!(parse_descriptors(&json!([valid.clone(), valid])).is_err());
    let registry = GraphNodeRegistry::default();
    assert!(registry
        .register_steps("other", &[descriptor()], Fixture::new(1))
        .is_err());
    registry
        .register_steps("fixture", &[descriptor()], Fixture::new(1))
        .unwrap();
    let retained = registry.step("fixture.generate").unwrap();
    registry.unregister("fixture");
    assert!(registry.step("fixture.generate").is_none());
    let request = request("removed");
    let call = StepCall {
        request: &request,
        job: None,
        deadline: Instant::now() + Duration::from_secs(5),
        cancellation: Default::default(),
    };
    assert!(retained.adapter.submit(&call).is_err());
}

#[test]
fn plugin_steps_edges_and_port_cardinality_are_checked() {
    let descriptor = descriptor();
    assert!(descriptor.accepts_edge(&descriptor.outputs["result"], "prompt"));
    let image = StepPort {
        kind: PortKind::Image,
        min_items: 1,
        max_items: 1,
        schema: None,
    };
    assert!(!descriptor.accepts_edge(&image, "prompt"));
    assert!(descriptor
        .validate_values(&Default::default(), false)
        .is_err());
    let mut input = request("operation").inputs;
    input.get_mut("prompt").unwrap().push(PortValue::Text {
        text: "extra".into(),
    });
    assert!(descriptor.validate_values(&input, false).is_err());
}

#[test]
fn plugin_steps_waiting_approval_reestimates_exact_operation_without_duplicate_submit() {
    let request = request("approval");
    let ledger = ledger(&request, 3);
    let descriptor = descriptor();
    let fixture = Fixture::new(5);
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    let held = drive(&runtime, &request);
    assert_eq!(held.state, OperationState::WaitingForOwnerOk);
    assert_eq!(
        held.waiting.unwrap(),
        Waiting {
            reason: WaitingReason::OverRunCeiling,
            estimate: Some(5),
            remaining: 3,
            unit: "credit_milli".into()
        }
    );
    assert!(ledger
        .authorize_fresh("approval", &request.context.scope, "foreign", 5, 100)
        .is_err());
    ledger
        .authorize_fresh("approval", &request.context.scope, "owner", 4, 100)
        .unwrap();
    assert_eq!(
        drive(&runtime, &request).waiting.unwrap().reason,
        WaitingReason::OverStepAllowance
    );
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 0);
    ledger
        .authorize_fresh("approval", &request.context.scope, "owner", 5, 100)
        .unwrap();
    assert_eq!(drive(&runtime, &request).reserved, 5);
    assert_eq!(drive(&runtime, &request).job.as_deref(), Some("job-1"));
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 1);
    let mut next = request.clone();
    next.context.operation = "next".into();
    next.context.node = "next".into();
    assert_eq!(
        drive(&runtime, &next).state,
        OperationState::WaitingForOwnerOk
    );
    assert_eq!(ledger.history(&request.context.scope).unwrap().len(), 2);
}

#[test]
fn plugin_steps_missing_and_stale_estimates_remain_distinct_owner_holds() {
    for (estimate, reason) in [
        (None, WaitingReason::EstimateMissing),
        (
            Some(Estimate {
                amount: 1,
                unit: "credit_milli".into(),
                valid_until_ms: 99,
            }),
            WaitingReason::EstimateStale,
        ),
    ] {
        let request = request("estimate");
        let ledger = ledger(&request, 10);
        let descriptor = descriptor();
        let fixture = Fixture::new(1);
        *fixture.estimate.lock() = estimate;
        let runtime = runtime(&ledger, &descriptor, fixture.clone());
        let held = drive(&runtime, &request);
        assert_eq!(held.state, OperationState::WaitingForOwnerOk);
        assert_eq!(held.waiting.unwrap().reason, reason);
        assert_eq!(fixture.submitted.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn plugin_steps_atomic_units_have_independent_run_ceilings() {
    let request = request("fractional-credit");
    let ledger = ledger(&request, 1);
    let descriptor = descriptor();
    let fixture = Fixture::new(1);
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    assert_eq!(drive(&runtime, &request).reserved, 1);
    ledger
        .admit_run(&request.context.scope, "usd_micro", 2)
        .unwrap();
    let mut dollars = request.clone();
    dollars.context.operation = "fractional-dollar".into();
    dollars.context.node = "fractional-dollar".into();
    dollars.unit = "usd_micro".into();
    *fixture.estimate.lock() = Some(Estimate {
        amount: 2,
        unit: "usd_micro".into(),
        valid_until_ms: 200,
    });
    assert_eq!(drive(&runtime, &dollars).reserved, 2);
    assert!(ledger
        .admit_run(&request.context.scope, "credit_milli", 2)
        .is_err());
    assert!(ledger
        .admit_run(&request.context.scope, "usd_micro", MAX_COST_AMOUNT + 1)
        .is_err());
}

#[test]
fn plugin_steps_changed_inputs_or_revision_cannot_reuse_owner_approval() {
    let request = request("immutable");
    let ledger = ledger(&request, 0);
    let descriptor = descriptor();
    let fixture = Fixture::new(1);
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    drive(&runtime, &request);
    ledger
        .authorize_fresh("immutable", &request.context.scope, "owner", 1, 100)
        .unwrap();
    for change in 0..3 {
        let mut changed = request.clone();
        match change {
            0 => changed.params = json!({"model":"large"}),
            1 => {
                changed.inputs.get_mut("prompt").unwrap()[0] = PortValue::Text {
                    text: "changed".into(),
                }
            }
            _ => changed.revision = 2,
        }
        assert!(runtime
            .drive(
                "fixture",
                &changed,
                || 100,
                Instant::now() + Duration::from_secs(5),
                Default::default()
            )
            .is_err());
    }
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 0);
}

#[test]
fn plugin_steps_uncertain_submission_survives_restart_without_secret_trace_or_replay() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("operations.sqlite");
    let request = request("uncertain");
    let descriptor = descriptor();
    let fixture = Fixture::new(4);
    *fixture.submit_result.lock() = Err("opaque-secret-token /private/path".into());
    {
        let ledger = OperationLedger::open(&path).unwrap();
        ledger
            .admit_run(&request.context.scope, &request.unit, 10)
            .unwrap();
        let runtime = runtime(&ledger, &descriptor, fixture.clone());
        assert_eq!(drive(&runtime, &request).state, OperationState::Unknown);
    }
    let ledger = OperationLedger::open(&path).unwrap();
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    let record = drive(&runtime, &request);
    assert_eq!(record.state, OperationState::Unknown);
    assert_eq!(record.reserved, 4);
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 1);
    let trace = serde_json::to_string(&ledger.history(&request.context.scope).unwrap()).unwrap();
    assert!(!trace.contains("opaque-secret-token"));
    assert!(!trace.contains("/private/path"));
    let recovered = runtime
        .reconcile(
            &request,
            Instant::now() + Duration::from_secs(5),
            Default::default(),
        )
        .unwrap();
    assert_eq!(recovered.state, OperationState::Succeeded);
    assert_eq!(recovered.job.as_deref(), Some("job-1"));
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 1);
}

#[test]
fn plugin_steps_persisted_submitting_is_unknown_and_job_identity_never_replaced() {
    let request = request("submitting");
    let ledger = ledger(&request, 10);
    let descriptor = descriptor();
    let fixture = Fixture::new(1);
    ledger.prepare("fixture", &request).unwrap();
    ledger
        .reserve(
            "submitting",
            Some(Estimate {
                amount: 1,
                unit: request.unit.clone(),
                valid_until_ms: 200,
            }),
            100,
        )
        .unwrap();
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    assert_eq!(drive(&runtime, &request).state, OperationState::Unknown);
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 0);
    *fixture.status_result.lock() = Ok(result(OperationState::Queued));
    runtime
        .reconcile(
            &request,
            Instant::now() + Duration::from_secs(5),
            Default::default(),
        )
        .unwrap();
    let mut replacement = result(OperationState::Succeeded);
    replacement.job = "replacement".into();
    *fixture.status_result.lock() = Ok(replacement);
    let record = runtime
        .reconcile(
            &request,
            Instant::now() + Duration::from_secs(5),
            Default::default(),
        )
        .unwrap();
    assert_eq!(record.state, OperationState::Unknown);
    assert_eq!(record.job.as_deref(), Some("job-1"));
}

#[test]
fn plugin_steps_cancel_retains_effect_reservation_and_prior_outputs() {
    let request = request("cancel");
    let ledger = ledger(&request, 10);
    let descriptor = descriptor();
    let fixture = Fixture::new(3);
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    drive(&runtime, &request);
    let canceled = runtime
        .cancel(&request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert_eq!(canceled.state, OperationState::Canceled);
    assert_eq!(canceled.reserved, 3);
    assert_eq!(drive(&runtime, &request).state, OperationState::Canceled);
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 1);
    let mut next = request.clone();
    next.context.operation = "complete".into();
    next.context.node = "complete".into();
    drive(&runtime, &next);
    let completed = runtime
        .reconcile(
            &next,
            Instant::now() + Duration::from_secs(5),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .cancel(&next, Instant::now() + Duration::from_secs(5))
            .unwrap()
            .outputs,
        completed.outputs
    );
}

#[test]
fn plugin_steps_cancellation_before_submit_never_calls_adapter() {
    let request = request("canceled");
    let ledger = ledger(&request, 10);
    let descriptor = descriptor();
    let fixture = Fixture::new(1);
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    let cancellation = crate::converter::ConversionCancellation::default();
    cancellation.cancel();
    let record = runtime
        .drive(
            "fixture",
            &request,
            || 100,
            Instant::now() + Duration::from_secs(5),
            cancellation,
        )
        .unwrap();
    assert_eq!(record.state, OperationState::Canceled);
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 0);
}

#[test]
fn plugin_steps_estimate_freshness_uses_admission_time_and_unsupported_cancel_is_unknown() {
    let request = request("freshness");
    let ledger = ledger(&request, 10);
    let mut descriptor = descriptor();
    let fixture = Fixture::new(1);
    descriptor.lifecycle.cancel = false;
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    let held = runtime
        .drive(
            "fixture",
            &request,
            || 201,
            Instant::now() + Duration::from_secs(5),
            Default::default(),
        )
        .unwrap();
    assert_eq!(held.waiting.unwrap().reason, WaitingReason::EstimateStale);
    assert!(ledger
        .authorize_fresh("freshness", &request.context.scope, "owner", 1, 201)
        .is_err());
    fixture.estimate.lock().as_mut().unwrap().valid_until_ms = 300;
    runtime
        .refresh_estimate(
            &request,
            || 202,
            Instant::now() + Duration::from_secs(5),
            Default::default(),
        )
        .unwrap();
    ledger
        .authorize_fresh("freshness", &request.context.scope, "owner", 1, 202)
        .unwrap();
    assert_eq!(
        runtime
            .drive(
                "fixture",
                &request,
                || 202,
                Instant::now() + Duration::from_secs(5),
                Default::default()
            )
            .unwrap()
            .state,
        OperationState::Queued
    );
    let canceled = runtime
        .cancel(&request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert_eq!(canceled.state, OperationState::Unknown);
    assert_eq!(canceled.reserved, 1);
    assert_eq!(drive(&runtime, &request).state, OperationState::Unknown);
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 1);
}

#[test]
fn plugin_steps_concurrent_reservations_submit_and_charge_at_most_once() {
    review::reserve_pre_submit_cancel();
}

#[test]
fn plugin_steps_native_js_adapter_parity_uses_identical_contract_fixtures() {
    let vm=Arc::new(crate::js_runtime::PluginVm::from_source("blueprint-parity", r#"module.exports={
        blueprintEstimate:()=>({amount:2,unit:'credit_milli',validUntilMs:200}),
        blueprintSubmit:(r)=>{
            if(r.context.operation!=='parity'||r.context.scope.team!=='team'||r.nodeType!=='fixture.generate'||r.unit!=='credit_milli'||!r.deadlineMs||r.job!=='job-1') throw Error('invalid fixture context');
            return {job:'job-1',state:'queued',outcome:null,outputs:{}};
        },
        blueprintReconcile:()=>({job:'job-1',state:'succeeded',outcome:'done',outputs:{result:[{kind:'text',text:'fixture output'}]}}),
        blueprintCancel:()=>({job:'job-1',state:'canceled',outcome:null,outputs:{}})
    };"#.into()).unwrap());
    let js = crate::js_runtime::JsBlueprintStep::new(vm);
    let native = Fixture::new(2);
    let request = request("parity");
    let call = StepCall {
        request: &request,
        job: Some("job-1"),
        deadline: Instant::now() + Duration::from_secs(5),
        cancellation: Default::default(),
    };
    assert_eq!(js.estimate(&call).unwrap(), native.estimate(&call).unwrap());
    assert_eq!(js.submit(&call).unwrap(), native.submit(&call).unwrap());
    assert_eq!(
        js.reconcile(&call).unwrap(),
        native.reconcile(&call).unwrap()
    );
    assert_eq!(js.cancel(&call).unwrap(), native.cancel(&call).unwrap());
}

mod media;
mod monitor;
mod refresh;
mod review;
