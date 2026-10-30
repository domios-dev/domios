use std::sync::Arc;

use serde_json::{json, Value};

use super::{NO_AGENT_CALLER, SURFACE_DENIED};
use crate::blueprint_step::*;
use crate::host_traits::PluginHostCtx;
use crate::js_runtime::host_api::PluginSandbox;
use crate::js_runtime::host_compute::tests::AnalyticsHost;
use crate::js_runtime::manifest::PermissionsConfig;
use crate::js_runtime::vm::{call_json, PluginVm};
use crate::js_runtime::JsBlueprintStep;

// Each entry point reports what host.surface.call answered, so the fake host's echo
// (`plugin|caller|verb|args`) shows which caller core received.
const STEP_PROBE: &str = r#"function answer(req){var r=JSON.parse(host.surface.call("status",JSON.stringify(["--of","forged"])));
    return {job:req.context.operation,state:'succeeded',outcome:'done',outputs:{text:[{kind:'text',text:r.result||r.error}]}};}
module.exports.default={blueprintSubmit:answer,blueprintReconcile:answer,blueprintCancel:answer,
    outside(){return JSON.parse(host.surface.call("status","[]"));}};"#;

fn step_probe(surfaces: bool) -> (Arc<PluginVm>, Arc<dyn PluginHostCtx>) {
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms(
            "step-probe",
            &PermissionsConfig {
                surfaces,
                ..Default::default()
            },
        ),
    );
    let vm = Arc::new(
        PluginVm::from_source_sandboxed("step-probe", STEP_PROBE.into(), None, sandbox).unwrap(),
    );
    let host: Arc<dyn PluginHostCtx> = Arc::new(AnalyticsHost::default());
    assert!(vm.set_service(Arc::downgrade(&host)));
    (vm, host)
}

fn request(owner: &str) -> StepRequest {
    StepRequest {
        context: StepContext {
            scope: RunScope {
                workspace: "w".into(),
                team: None,
                run: "r".into(),
            },
            owner: owner.into(),
            node: "read".into(),
            attempt: "attempt".into(),
            operation: "operation".into(),
            node_label: String::new(),
            input_lineage: vec![],
        },
        node_type: "step-probe.read".into(),
        revision: 1,
        params: json!({"owner":"forged-tab"}),
        inputs: Default::default(),
    }
}

fn answered(update: JobUpdate) -> String {
    match update.outputs["text"].as_slice() {
        [PortValue::Text { text }] => text.clone(),
        other => panic!("unexpected outputs {other:?}"),
    }
}

fn call(request: &StepRequest) -> StepCall<'_> {
    StepCall {
        request,
        job: Some("job"),
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(5),
        cancellation: Default::default(),
    }
}

#[test]
fn blueprint_step_surface_call_acts_as_run_owner() {
    let (vm, _host) = step_probe(true);
    let request = request("owner-tab");
    let step = JsBlueprintStep::new(vm.clone());
    let text = answered(step.submit(&call(&request)).unwrap());
    assert_eq!(
        text,
        r#"step-probe|owner-tab|status|["--of","forged"]"#,
        "the surface caller is the host-stamped owner, whatever the params or args say"
    );
    let after: Value = call_json(&vm, "outside", &[]).unwrap();
    assert_eq!(after["error"], NO_AGENT_CALLER, "the scope ends with the submit");
}

#[test]
fn surface_call_outside_agent_command_or_step_is_refused() {
    let (vm, _host) = step_probe(true);
    let outside: Value = call_json(&vm, "outside", &[]).unwrap();
    assert_eq!(outside["error"], NO_AGENT_CALLER);
    let direct: Value = call_json(
        &vm,
        "blueprintSubmit",
        &[serde_json::to_value(request("owner-tab")).unwrap()],
    )
    .unwrap();
    assert_eq!(
        direct["outputs"]["text"][0]["text"], NO_AGENT_CALLER,
        "the export name alone grants nothing; only the step adapter stamps the owner"
    );
    let request = request("owner-tab");
    let step = JsBlueprintStep::new(vm);
    assert_eq!(answered(step.reconcile(&call(&request)).unwrap()), NO_AGENT_CALLER);
    assert_eq!(answered(step.cancel(&call(&request)).unwrap()), NO_AGENT_CALLER);
}

#[test]
fn blueprint_step_surface_call_requires_surfaces_permission() {
    let (vm, _host) = step_probe(false);
    let request = request("owner-tab");
    let text = answered(JsBlueprintStep::new(vm).submit(&call(&request)).unwrap());
    assert_eq!(text, SURFACE_DENIED);
}
