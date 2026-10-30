use super::tests::test_caps;
use super::*;
use crate::blueprint_step::*;
use serde_json::json;

fn manifest() -> PluginManifest {
    serde_json::from_value(json!({"id":"fixture","version":"0","displayName":"Fixture","hostApi":crate::js_runtime::host_api::HOST_API_VERSION,"entry":"plugin.js",
        "capabilities":{"graphNodes":{"steps":[{"contractVersion":1,"revision":1,"id":"fixture.generate","title":"Generate","description":"Fixture",
            "params":{"type":"object","properties":{}},"inputs":{},"outputs":{},"outcomes":["done","failed"],
            "effects":{"sideEffects":true},"lifecycle":{"reconcile":true,"cancel":true}}]}}})).unwrap()
}

#[test]
fn plugin_identity_preconstructed_capabilities_cannot_replace_vm_identity() {
    let caps = test_caps();
    let vm = Arc::new(
        super::super::PluginVm::from_source("fixture", "module.exports.default={};".into())
            .unwrap(),
    );
    for id in [
        "blueprint-plugin-operations.sqlite",
        "blueprint-run-media.sqlite",
        "other-valid-id",
        "nul",
    ] {
        let mut manifest = manifest();
        manifest.id = id.into();
        manifest.capabilities.view = true;
        caps.register_plugin(&manifest, &vm);
        assert!(caps.view.get(id).is_none());
    }
}

#[test]
fn plugin_steps_registration_requires_declared_exports_and_revokes_snapshots() {
    let caps = test_caps();
    let manifest = manifest();
    let incomplete = Arc::new(
        super::super::PluginVm::from_source(
            "fixture",
            "module.exports={blueprintSubmit:()=>null};".into(),
        )
        .unwrap(),
    );
    caps.register_plugin(&manifest, &incomplete);
    assert!(caps.graph_nodes.step("fixture.generate").is_none());
    let complete = Arc::new(
        super::super::PluginVm::from_source(
            "fixture",
            r#"module.exports={
        blueprintSubmit:()=>({job:'job',state:'queued',outcome:null,outputs:{}}),
        blueprintReconcile:()=>({job:'job',state:'running',outcome:null,outputs:{}}),
        blueprintCancel:()=>({job:'job',state:'canceled',outcome:null,outputs:{}})
    };"#
            .into(),
        )
        .unwrap(),
    );
    caps.register_plugin(&manifest, &complete);
    let snapshot = caps.graph_nodes.step("fixture.generate").unwrap();
    caps.deregister_plugin("fixture");
    assert!(caps.graph_nodes.steps().is_empty());
    let request = StepRequest {
        context: StepContext {
            scope: RunScope {
                workspace: "w".into(),
                team: None,
                run: "r".into(),
            },
            owner: "owner".into(),
            node: "node".into(),
            attempt: "attempt".into(),
            operation: "operation".into(),
            node_label: String::new(),
            input_lineage: vec![],
        },
        node_type: "fixture.generate".into(),
        revision: 1,
        params: json!({}),
        inputs: Default::default(),
    };
    let call = StepCall {
        request: &request,
        job: None,
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(1),
        cancellation: Default::default(),
    };
    assert!(snapshot.adapter.submit(&call).is_err());
}
