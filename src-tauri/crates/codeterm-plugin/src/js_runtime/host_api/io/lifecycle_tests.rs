use super::*;
use crate::host_traits::PluginHostCtx;
use crate::js_runtime::host_compute::tests::AnalyticsHost;
use crate::js_runtime::manifest::PermissionsConfig;
use crate::js_runtime::vm::{call_json, PluginVm};

fn fixture() -> (PluginVm, Arc<dyn PluginHostCtx>) {
    configured_fixture(false)
}

fn configured_fixture(surfaces: bool) -> (PluginVm, Arc<dyn PluginHostCtx>) {
    let id = "io-lifecycle";
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms(id, &PermissionsConfig {surfaces, ..Default::default()}),
    );
    let vm = PluginVm::from_source_sandboxed(
        id,
        r#"module.exports = {
          request(request) { return host.io.run((function* () {
            try { return yield request; } catch (error) { return {error:String(error)}; }
          })()); },
          surface() { return host.io.run((function* () { return JSON.parse(yield* host.io.surface.call('status','[]')); })()); },
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

#[test]
fn surface_jobs_capture_the_agent_caller_and_do_not_leak_it() {
    let (vm, _service) = configured_fixture(true);
    {
        let _caller = crate::js_runtime::host_surface::AgentCallerScope::enter("own-agent");
        let result: Value = call_json(&vm, "surface", &[]).unwrap();
        assert_eq!(result["result"], "io-lifecycle|own-agent|status|[]");
        let reply: String = call_json(&vm, "request", &[json!({"op":"surfaceCall","verb":"view-probe","args":"[]"})]).unwrap();
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["error"], "host.io surface calls require a native surface runtime");
    }
    let result: Value = call_json(&vm, "surface", &[]).unwrap();
    assert_eq!(result["error"], crate::js_runtime::host_surface::NO_AGENT_CALLER);
}

#[test]
fn surface_jobs_retain_the_original_blueprint_owner() {
    use crate::blueprint_step::{StepCall, StepInputScope};
    let (vm, _service) = configured_fixture(true);
    let request = crate::js_runtime::host_surface::step_tests::request("run-owner");
    let call = StepCall {
        request: &request,
        job: None,
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(5),
        cancellation: Default::default(),
    };
    let _step = StepInputScope::enter("io-lifecycle", &call, true);
    let _caller = crate::js_runtime::host_surface::AgentCallerScope::enter("other-agent");
    let result: Value = call_json(&vm, "surface", &[]).unwrap();
    assert_eq!(result["result"], "io-lifecycle|run-owner|status|[]");
}

#[test]
fn surface_and_event_jobs_preserve_their_grants() {
    let (vm, _service) = fixture();
    let _caller = crate::js_runtime::host_surface::AgentCallerScope::enter("own-agent");
    let result: Value = call_json(&vm, "surface", &[]).unwrap();
    assert_eq!(result["error"], crate::js_runtime::host_surface::SURFACE_DENIED);
    let result: Value = call_json(&vm, "request", &[json!({"op":"graphEventEmit","event":{}})]).unwrap();
    assert!(result["error"].as_str().unwrap().contains("graph_events permission required"));
}
