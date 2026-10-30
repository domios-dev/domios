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
fn provider_manifests_register_real_run_prompt_steps() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
    let mut registered = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let directory = entry.unwrap().path();
        let Ok(manifest_json) = std::fs::read_to_string(directory.join("plugin.json")) else {
            continue;
        };
        let raw: serde_json::Value = serde_json::from_str(&manifest_json).unwrap();
        if !raw
            .pointer("/capabilities/graphNodes/steps")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|steps| {
                steps.iter().any(|step| {
                    step["id"]
                        .as_str()
                        .is_some_and(|id| id.ends_with(".run_prompt"))
                })
            })
        {
            continue;
        }
        let manifest: PluginManifest = serde_json::from_str(&manifest_json).unwrap();
        let source = std::fs::read_to_string(directory.join("plugin.js")).unwrap();
        let vm = Arc::new(super::super::PluginVm::from_source(&manifest.id, source).unwrap());
        let caps = test_caps();
        caps.register_plugin(&manifest, &vm);
        let steps = caps.graph_nodes.steps();
        assert_eq!(steps.len(), 1);
        let descriptor = &steps[0].descriptor;
        assert_eq!(descriptor.id, format!("{}.run_prompt", manifest.id));
        assert_eq!(descriptor.outcomes, ["done", "failed"]);
        assert!(descriptor.lifecycle.reconcile && descriptor.lifecycle.cancel);
        assert!(manifest.permissions.spawn_agents);
        registered += 1;
    }
    assert_eq!(registered, 3);
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

#[test]
fn browser_manifest_declares_read_page_step() {
    use crate::host_traits::PluginHostCtx;
    let manifest = PluginManifest::from_json_str(include_str!(
        "../../../../../../plugins/browser/plugin.json"
    ))
    .expect("browser manifest parses");
    let sandbox = Arc::new(
        crate::js_runtime::host_api::PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms("browser", &manifest.permissions),
    );
    let vm = Arc::new(
        super::super::PluginVm::from_source_sandboxed(
            "browser",
            include_str!("../../../../../../plugins/browser/plugin.js").into(),
            None,
            sandbox,
        )
        .unwrap(),
    );
    let host: Arc<dyn PluginHostCtx> =
        Arc::new(crate::js_runtime::host_compute::tests::AnalyticsHost::default());
    assert!(vm.set_service(Arc::downgrade(&host)));
    let caps = test_caps();
    caps.register_plugin(&manifest, &vm);
    let steps = caps.graph_nodes.steps();
    assert_eq!(
        steps
            .iter()
            .map(|step| step.descriptor.id.as_str())
            .collect::<Vec<_>>(),
        ["browser.read_page"]
    );
    let step = &steps[0];
    assert_eq!(step.plugin, "browser");
    assert_eq!(
        step.descriptor.outcomes,
        ["read", "navigation_failed", "needs_human", "browser_busy"]
    );
    assert_eq!(step.descriptor.inputs.keys().collect::<Vec<_>>(), ["url"]);
    assert_eq!(
        step.descriptor.outputs.keys().collect::<Vec<_>>(),
        ["final_url", "text"]
    );
    assert!(!step.descriptor.lifecycle.reconcile && !step.descriptor.lifecycle.cancel);
    // The fake host answers status as a live browser, so the real bundle stops at browser_busy.
    let mut request = StepRequest {
        context: StepContext {
            scope: RunScope {
                workspace: "w".into(),
                team: None,
                run: "r".into(),
            },
            owner: "owner-tab".into(),
            node: "read".into(),
            attempt: "attempt".into(),
            operation: "operation".into(),
            node_label: String::new(),
            input_lineage: vec![],
        },
        node_type: "browser.read_page".into(),
        revision: 1,
        params: json!({"url":"https://example.com"}),
        inputs: Default::default(),
    };
    let call = |request: &StepRequest| {
        step.adapter
            .submit(&StepCall {
                request,
                job: None,
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(5),
                cancellation: Default::default(),
            })
            .unwrap()
    };
    let busy = call(&request);
    assert_eq!(busy.state, OperationState::Succeeded);
    assert_eq!(busy.outcome.as_deref(), Some("browser_busy"));
    step.descriptor
        .validate_values(&busy.outputs, true)
        .unwrap();
    request.params = json!({"url":"file:///etc/passwd"});
    assert_eq!(call(&request).outcome.as_deref(), Some("navigation_failed"));
}
