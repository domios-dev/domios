use super::*;
use crate::host_traits::PluginHostCtx;
use crate::js_runtime::host_compute::tests::AnalyticsHost;
use crate::js_runtime::manifest::PermissionsConfig;
use crate::js_runtime::vm::{call_json, PluginVm};

fn fixture() -> (PluginVm, Arc<dyn PluginHostCtx>) {
    let id = "io-lifecycle";
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms(id, &PermissionsConfig::default()),
    );
    let vm = PluginVm::from_source_sandboxed(
        id,
        r#"module.exports = {
          request(request) { return host.io.run((function* () {
            try { return yield request; } catch (error) { return {error:String(error)}; }
          })()); },
          legacy() { return host.progress({label:'denied'}); }
        };"#
        .into(),
        Some(
            json!({"id":id,"version":"0.0.0","displayName":"Fixture","hostIoJobs":true})
                .to_string(),
        ),
        sandbox,
    )
    .unwrap();
    let service: Arc<dyn PluginHostCtx> = Arc::new(AnalyticsHost::default());
    assert!(vm.set_service(Arc::downgrade(&service)));
    (vm, service)
}

#[test]
fn lifecycle_jobs_preserve_progress_result_and_refuse_legacy_progress() {
    let (vm, _service) = fixture();
    assert_eq!(
        call_json::<Value>(
            &vm,
            "request",
            &[json!({"op":"progress","options":{"label":"working"}})]
        )
        .unwrap(),
        ""
    );
    assert!(
        matches!(call_json::<Value>(&vm, "legacy", &[]), Err(VmError::Script(message)) if message.contains("requires a host.io job"))
    );
}

#[test]
fn lifecycle_recording_jobs_preserve_the_plugin_grant() {
    let (vm, _service) = fixture();
    let refused: Value = call_json(
        &vm,
        "request",
        &[json!({"op":"localRecordingSetEnabled","enabled":true})],
    )
    .unwrap();
    assert!(refused["error"]
        .as_str()
        .unwrap()
        .contains("local_recording_grant_required"));
    let status: Value = call_json(&vm, "request", &[json!({"op":"localRecordingStatus"})]).unwrap();
    assert_eq!(status["error"], "trigger_not_declared");
    let report: Value = call_json(&vm, "request", &[json!({"op":"receiverStatusReport"})]).unwrap();
    assert!(report["error"]
        .as_str()
        .unwrap()
        .contains("local_recording_grant_required"));
}

#[test]
fn lifecycle_stream_start_preserves_network_confinement() {
    let (vm, _service) = fixture();
    let refused: Value = call_json(&vm, "request", &[json!({"op":"fetchStreamStart","options":{"url":"https://denied.invalid/","method":"GET"}})]).unwrap();
    assert!(refused["error"].as_str().unwrap().contains("fetch denied"));
}

#[test]
fn lifecycle_poll_and_close_never_claim_foreign_jobs() {
    let (vm, _service) = fixture();
    for op in ["execPoll", "fetchStreamPoll"] {
        let result: Value = call_json(&vm, "request", &[json!({"op":op,"job":"foreign"})]).unwrap();
        assert!(result.get("error").is_some());
    }
    for op in ["execClose", "fetchStreamClose"] {
        assert_eq!(
            call_json::<Value>(&vm, "request", &[json!({"op":op,"job":"foreign"})]).unwrap(),
            Value::Null
        );
    }
}
