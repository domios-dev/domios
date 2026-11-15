use crate::host_traits::PluginHostCtx;
use crate::js_runtime::{
    host_api::PluginSandbox,
    host_compute::tests::AnalyticsHost,
    manifest::PermissionsConfig,
    vm::{call_json, PluginVm},
};
use serde_json::{json, Value};
use std::sync::Arc;

fn fixture(source: String, root: &std::path::Path) -> (Arc<PluginVm>, Arc<dyn PluginHostCtx>) {
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![root.to_path_buf()], vec![]).with_identity_and_perms(
            "origin-fixture",
            &PermissionsConfig {
                surfaces: true,
                agent_commands: true,
                ..Default::default()
            },
        ),
    );
    let vm =
        Arc::new(PluginVm::from_source_sandboxed("origin-fixture", source, None, sandbox).unwrap());
    let host: Arc<dyn PluginHostCtx> = Arc::new(AnalyticsHost::default());
    assert!(vm.set_service(Arc::downgrade(&host)));
    (vm, host)
}

#[test]
fn surface_continuations_refuse_foreign_jobs_and_stale_root_markers_without_consuming_receipts() {
    let root = tempfile::tempdir().unwrap();
    let source = r#"module.exports.default={saved:null,
        seed(job){this.saved=host.awaitJob(job,function(){return 'must not resume';});return 'saved';},
        replay(){return this.saved;},foreign(job){return host.awaitJob(job,function(){throw Error('foreign callback executed');});},
        steal(k){return typeof globalThis.__ct_await_take__(k);}
    };"#;
    let (vm, _host) = fixture(source.into(), root.path());
    let owned = crate::js_runtime::host_exec::start_retained_json_job("origin-fixture", || {
        json!({"value":42}).to_string()
    })
    .unwrap();
    assert_eq!(
        call_json::<String>(&vm, "seed", &[json!(owned)]).unwrap(),
        "saved"
    );
    assert!(call_json::<Value>(&vm, "replay", &[]).is_err());
    assert_eq!(
        call_json::<String>(&vm, "steal", &[json!("1")]).unwrap(),
        "undefined"
    );
    let foreign = crate::js_runtime::host_exec::start_retained_json_job("other-plugin", || {
        json!({"value":7}).to_string()
    })
    .unwrap();
    assert!(call_json::<Value>(&vm, "foreign", &[json!(foreign)]).is_err());
    for (plugin, job, expected) in [
        ("origin-fixture", &owned, 42),
        ("other-plugin", &foreign, 7),
    ] {
        let crate::js_runtime::host_exec::WaitResult::Completed(value) =
            crate::js_runtime::host_exec::wait_json(plugin, job)
        else {
            panic!("known receipt disappeared");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&value).unwrap()["value"],
            expected
        );
    }
}

#[test]
fn surface_continuation_consumes_a_same_root_receipt_only_once() {
    let root = tempfile::tempdir().unwrap();
    let (vm,_host) = fixture(r#"module.exports.default={count:0,replay(job){var self=this,marker;marker=host.awaitJob(job,function(){self.count++;return marker;});return marker;},counted(){return this.count;}};"#.into(),root.path());
    let job = crate::js_runtime::host_exec::start_retained_json_job("origin-fixture", || {
        json!({"value":1}).to_string()
    })
    .unwrap();
    assert!(call_json::<Value>(&vm, "replay", &[json!(job)]).is_err());
    assert_eq!(call_json::<u32>(&vm, "counted", &[]).unwrap(), 1);
}
