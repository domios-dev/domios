use super::*;

fn vm_with(src: &str) -> Arc<PluginVm> {
    Arc::new(PluginVm::from_source("test-view", src.to_string()).expect("vm"))
}

#[tokio::test]
async fn routes_bridge_calls_to_view_call() {
    let vm = vm_with(
        r#"module.exports.default = {
            viewCall(method, args) {
                if (method === "echo") return { got: args.x * 2 };
                return { error: "unknown" };
            }
        }"#,
    );
    let v = JsViewProvider::new(vm, "test-view");
    let out = v
        .call(
            "echo",
            serde_json::json!({ "x": 21 }),
            &crate::view_host::ViewCallContext::panel("test-view"),
        )
        .await
        .unwrap();
    assert_eq!(out["got"], 42);
}

/// NEGATIVE CONTROL for the grant seam: view-call args are iframe-authored
/// (the host stamps cwd only when the iframe omits it), so an `args.cwd`
/// must never mint a read grant — otherwise any drop-in with a view could
/// self-issue read access to arbitrary paths. Reads under a tab cwd work
/// only after a host-driven bubble/glance call granted that root.
#[tokio::test]
async fn view_args_cwd_never_mints_a_read_grant() {
    use crate::js_runtime::host_api::PluginSandbox;
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("loot")).unwrap();
    let sandbox = Arc::new(PluginSandbox::restricted(vec![], vec![]));
    let vm = Arc::new(
        PluginVm::from_source_sandboxed(
            "confined-view",
            r#"module.exports.default = {
                viewCall(method, args) { return { names: host.listChildDirs(args.cwd) }; }
            }"#
            .to_string(),
            None,
            sandbox.clone(),
        )
        .expect("vm"),
    );
    let v = JsViewProvider::new(vm, "confined-view");
    let cwd = tmp.path().to_string_lossy().into_owned();

    let out = v
        .call(
            "probe",
            serde_json::json!({ "cwd": cwd }),
            &crate::view_host::ViewCallContext::panel("confined-view"),
        )
        .await
        .unwrap();
    assert_eq!(
        out["names"],
        serde_json::Value::Null,
        "an iframe-supplied cwd must stay denied"
    );

    // Once the HOST granted the same cwd (bubble/glance path), the view works.
    sandbox.grant_cwd_read_root(&cwd);
    let out = v
        .call(
            "probe",
            serde_json::json!({ "cwd": cwd }),
            &crate::view_host::ViewCallContext::panel("confined-view"),
        )
        .await
        .unwrap();
    assert_eq!(out["names"], serde_json::json!(["loot"]));
}
