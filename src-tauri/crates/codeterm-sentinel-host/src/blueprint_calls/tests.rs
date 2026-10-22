use super::*;
#[test]
fn blueprint_v2_call_context_retains_exact_parent_identity() {
    let mut context = CallContext::default();
    let pin = CallPin {
        id: "mem:callee".into(),
        hash: "a".repeat(64),
    };
    context
        .enter("call-node", &pin, JournalMarker::Step(4))
        .unwrap();
    let parent = context.parent().unwrap();
    assert_eq!(parent.marker, JournalMarker::Step(4));
    assert_eq!(parent.frame.id, "mem:callee");
    assert_eq!(parent.frame.hash, "a".repeat(64));
    assert_eq!(
        context.enter("recursive", &pin, JournalMarker::Step(5)),
        Err(JournalReason::ContractViolation)
    );
    context.exit();
    assert!(context.parent().is_none());
}

use crate::pipeline::StepExecution;
use crate::store::{Lease, Owner, Store};
use codeterm_blueprint_types::blueprint_document::{self, Port};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::persistence_test_support::{close, ServiceInstance};
use std::sync::Arc;

fn port() -> Value {
    json!({"type":"object","properties":{"n":{"type":"integer"}},"required":["n"],"additionalProperties":false})
}
fn source(fail: bool) -> Document {
    let command: String = if fail {
        "exit 9".into()
    } else {
        if cfg!(windows) {
            "python -c \"import json,sys; p=json.load(sys.stdin); print(json.dumps({'n':p['value']['n']+1}))\"".into()
        } else {
            "python3 -c 'import json,sys; p=json.load(sys.stdin); print(json.dumps({\"n\":p[\"value\"][\"n\"]+1}))'".into()
        }
    };
    let value = json!({"schema_version":2,"kind":"pipeline","name":"real-call","owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"invoke","call":{"ref":"increment","with":{"p":{"lit":{"n":7}}}},"out":port()}],"blueprints":{"increment":{"inputs":{"p":port()},"steps":[{"id":"increment","run":{"lit":command},"in":port(),"out":port()},{"id":"fixed-sink","uses":"notify","with":{"payload":{"bind":"increment.out.n"}}}],"result":{"type":port(),"from":"increment.out"}}},"result":{"to":"notify"}});
    let source = serde_yaml::to_string(&value).unwrap();
    let closure = calls::resolve_local("fixture.yaml", &source).unwrap();
    assert!(calls::check_facts(&closure).1.is_empty());
    closure.document()
}
struct Receipt {
    output: Result<ExecutionEnvelope, JournalReason>,
    notices: Vec<String>,
    journal: Vec<codeterm_storage::BlueprintJournalEntry>,
    effects: Vec<SimulatedEffect>,
    stop: Option<CallStop>,
    view: Value,
}
fn run(document: Document, simulate: bool) -> Receipt {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let team = mux
        .resolve_or_mint_team_for_parent(&creator)
        .unwrap()
        .team_id;
    mux.get_tab_mut(&creator).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let service = &instance.fixture.service;
    let clock = codeterm_service_core::cron::clock::MockClock::starting_at(1000);
    let handle = crate::scheduler::fixture_handle_with_ui(
        Store::with_storage(&instance.storage, clock),
        service.ui.clone(),
    );
    crate::manage_fixture_ports(service, &handle);
    let owner = Owner {
        creator_tab_id: creator,
        team_id: Some(team),
        workspace_id: workspace,
        cwd: root.path().to_string_lossy().into_owned(),
    };
    // The fixture admits the static graph separately; depth cases exercise the runtime guard.
    let runtime = if document.nodes().any(|node| node.call.is_some()) {
        let mut projection = document.clone();
        projection.dependencies.clear();
        projection.blueprints.clear();
        projection.imports.clear();
        for node in &mut projection.steps {
            node.call = None;
            node.uses = Some(blueprint_document::NodeKind::Filter);
            node.params = BTreeMap::from([("pattern".into(), ValueForm::literal(".*"))]);
            node.output = None;
        }
        projection.lower().unwrap()
    } else {
        document.lower().unwrap()
    };
    let spec = serde_json::from_value(runtime["spec"].clone()).unwrap();
    let mut job = handle
        .store
        .lock()
        .prepare_pipeline(spec, owner, false)
        .unwrap();
    job.document = Some(json!(document));
    let job = crate::storage::commit(&handle.store, |store| store.stage_insert(job)).unwrap();
    let step = calls::lower::node(&document, &document.steps[0]).unwrap();
    let lease = Lease {
        job: job.clone(),
        cancelled: Arc::new(crate::probe::cancellation::Cancellation::new(false)),
        event: None,
    };
    let mut ctx = RunCtx::new(&handle, service, &lease, "call-proof");
    ctx.call_state.simulate = simulate;
    let result = execute_step(&mut ctx, &step, ExecutionEnvelope::from_text(""));
    let output = result.map(|step: StepExecution| step.output);
    let effects = ctx.call_state.effects.clone();
    let stop = ctx.call_state.stop.clone();
    drop(ctx);
    let journal = handle
        .store
        .lock()
        .typed_pipeline_log(&job.id, 200)
        .unwrap()
        .wait(crate::storage::COMMIT_DEADLINE)
        .unwrap()
        .value;
    let notices = service
        .notif
        .lock()
        .list(false)
        .iter()
        .filter_map(|n| n.body.clone())
        .collect();
    let view = project(&document, &journal).unwrap_or(Value::Null);
    drop(handle);
    drop(lease);
    close(instance);
    Receipt {
        output,
        notices,
        journal,
        effects,
        stop,
        view,
    }
}
#[test]
fn blueprint_v2_call_real_executor_delivers_eight_to_actual_local_notification_sink() {
    let receipt = run(source(false), false);
    assert_eq!(receipt.output.as_ref().unwrap().value, json!({"n":8}));
    assert_eq!(receipt.notices, vec!["8"]);
    assert!(receipt.effects.is_empty());
    let sink = receipt
        .journal
        .iter()
        .find(|row| row.step_name == "fixed-sink" && row.state == JournalState::Done)
        .unwrap();
    assert_eq!(sink.call_stack.len(), 1);
    assert_eq!(sink.call_stack[0].node, "invoke");
    assert!(sink.call_stack[0].id.contains("increment"));
    assert_eq!(sink.call_stack[0].hash.len(), 64);
    assert_eq!(
        receipt.view["expanded_calls"][0]["graph"]["steps"][0]["id"],
        "increment"
    );
    assert!(receipt.view["nested_run"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["step_name"] == "fixed-sink"
            && n["call_stack"][0]["hash"] == sink.call_stack[0].hash));
}
#[test]
fn blueprint_v2_call_simulate_records_nested_effect_provenance_and_delivers_zero() {
    let receipt = run(source(false), true);
    assert_eq!(receipt.output.unwrap().value, json!({"n":8}));
    assert!(receipt.notices.is_empty());
    assert_eq!(receipt.effects.len(), 1);
    assert_eq!(receipt.effects[0].effect["payload"], 8);
    assert_eq!(receipt.effects[0].call_stack[0].node, "invoke");
}
#[test]
fn blueprint_v2_call_nested_failure_stops_before_later_effect_and_retains_stack() {
    let receipt = run(source(true), false);
    assert!(receipt.output.is_err());
    assert!(receipt.notices.is_empty());
    assert!(!receipt
        .journal
        .iter()
        .any(|row| row.step_name == "fixed-sink"));
    let stop = receipt.stop.unwrap();
    assert_eq!(stop.node, "increment");
    assert_eq!(stop.call_stack[0].node, "invoke");
    assert!(receipt
        .journal
        .iter()
        .any(|row| row.step_name == "increment"
            && row.state == JournalState::Failed
            && !row.call_stack.is_empty()));
}
fn chain(depth: usize) -> Document {
    let object: Port = serde_json::from_value(port()).unwrap();
    let mut dependencies = BTreeMap::new();
    let mut next: Option<CallPin> = None;
    for index in (0..depth).rev() {
        let mut body:Document=serde_json::from_value(json!({"schema_version":2,"kind":"pipeline","name":format!("callee-{index}"),"inputs":{"p":port()},"steps":[{"id":"leaf","uses":"filter","with":{"pattern":{"lit":".*"}},"in":port(),"out":port()}],"result":{"type":port(),"from":"leaf.out"}})).unwrap();
        if let Some(pin) = next {
            body.steps[0].uses = None;
            body.steps[0].params.clear();
            body.steps[0].call = Some(Call {
                reference: pin.id.clone(),
                pin: Some(pin),
                arguments: BTreeMap::from([("p".into(), ValueForm::input("p"))]),
            });
        }
        let hash = blueprint_document::projection::identity(&body).unwrap();
        let callee = calls::FrozenCallee {
            id: format!("inline:callee-{index}"),
            hash,
            document: Box::new(body),
            locations: BTreeMap::new(),
            source_hash: None,
        };
        next = Some(callee.pin());
        dependencies.insert(callee.pin().key(), callee);
    }
    let mut root = source(false);
    root.dependencies = dependencies;
    root.blueprints.clear();
    root.steps[0].call.as_mut().unwrap().pin = next;
    root.steps[0].output = Some(object);
    root
}
#[test]
fn blueprint_v2_call_simulate_depth_eight_succeeds_depth_nine_budget_exhausted() {
    let eight = run(chain(8), true);
    assert_eq!(eight.output.unwrap().value, json!({"n":7}));
    assert!(eight.notices.is_empty());
    let nine = run(chain(9), true);
    assert_eq!(nine.output.unwrap_err(), JournalReason::BudgetExhausted);
    assert!(nine.notices.is_empty());
}
#[test]
fn blueprint_v2_call_callee_grant_widening_refuses_before_any_nested_claim() {
    let mut document = source(false);
    let key = document.dependencies.keys().next().unwrap().clone();
    let mut callee = document.dependencies.remove(&key).unwrap();
    callee.document.steps[0]
        .caps
        .push("net:forbidden.test".into());
    callee.hash = blueprint_document::projection::identity(&callee.document).unwrap();
    document.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
    document.dependencies.insert(callee.pin().key(), callee);
    let receipt = run(document, false);
    assert_eq!(receipt.output.unwrap_err(), JournalReason::SandboxDenied);
    assert!(receipt.notices.is_empty());
    assert!(!receipt
        .journal
        .iter()
        .any(|row| row.step_name == "increment"));
}
#[test]
fn blueprint_v2_call_out_of_scope_effect_target_refuses_before_nested_claim() {
    let mut document = source(false);
    let key = document.dependencies.keys().next().unwrap().clone();
    let mut callee = document.dependencies.remove(&key).unwrap();
    callee.document.steps[1].uses = Some(blueprint_document::NodeKind::Send);
    callee.document.steps[1]
        .params
        .insert("target".into(), ValueForm::literal("foreign-workspace-tab"));
    callee.hash = blueprint_document::projection::identity(&callee.document).unwrap();
    document.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
    document.dependencies.insert(callee.pin().key(), callee);
    let receipt = run(document, false);
    assert_eq!(
        receipt.output.unwrap_err(),
        JournalReason::ContractViolation
    );
    assert!(receipt.notices.is_empty());
    assert!(!receipt
        .journal
        .iter()
        .any(|row| row.step_name == "increment"));
}
