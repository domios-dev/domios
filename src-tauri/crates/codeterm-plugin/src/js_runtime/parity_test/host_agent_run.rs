use crate::{
    blueprint_step::*,
    host_traits::PluginHostCtx,
    js_runtime::{
        host_api::PluginSandbox,
        host_compute::tests::AnalyticsHost,
        manifest::PermissionsConfig,
        vm::{call_json, PluginVm},
        JsBlueprintStep,
    },
};
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn step_agent_run_native_js_parity_and_entrypoint_permission_bounds() {
    for allowed in [true, false] {
        let sandbox = Arc::new(
            PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms(
                "provider-fixture",
                &PermissionsConfig {
                    spawn_agents: allowed,
                    ..Default::default()
                },
            ),
        );
        let source = r#"function result(r,job){return {job:job,state:'succeeded',outcome:'done',outputs:{text:[{kind:'text',text:JSON.stringify(r)}]}};}
            module.exports.default={
            blueprintSubmit(r){return host.agentRun.start({prompt:'opaque'},function(s){return result(s,r.context.operation);});},
            blueprintReconcile(r){return host.agentRun.await(r.job,function(s){return result(s,r.job);});},
            blueprintCancel(r){return host.agentRun.cancel(r.job,function(s){return result(s,r.job);});},
            outside(){return host.agentRun.start({},function(s){return s;});}};"#;
        let vm = Arc::new(
            PluginVm::from_source_sandboxed("provider-fixture", source.into(), None, sandbox)
                .unwrap(),
        );
        let host: Arc<dyn PluginHostCtx> = Arc::new(AnalyticsHost::default());
        assert!(vm.set_service(Arc::downgrade(&host)));
        let outside: Value = call_json(&vm, "outside", &[]).unwrap();
        assert!(outside["error"].is_string());
        let request = crate::js_runtime::host_surface::step_tests::request("trusted-owner");
        let call = StepCall {
            request: &request,
            job: Some("run"),
            deadline: Instant::now() + Duration::from_secs(10),
            cancellation: Default::default(),
        };
        let step = JsBlueprintStep::new(vm);
        let decode = |update: JobUpdate| -> Value {
            match &update.outputs["text"][0] {
                PortValue::Text { text } => serde_json::from_str(text).unwrap(),
                _ => panic!("text expected"),
            }
        };
        let submitted = decode(step.submit(&call).unwrap());
        if allowed {
            let _scope = StepInputScope::enter("provider-fixture", &call, true);
            let scope = current_step_input("provider-fixture").unwrap();
            let native: Value =
                serde_json::from_str(&host.agent_run_start_json("provider-fixture", &scope, "{}"))
                    .unwrap();
            assert_eq!(submitted, native);
            assert_eq!(submitted, json!({"run":"provider-fixture:trusted-owner"}));
            drop(_scope);
            assert_eq!(
                decode(step.reconcile(&call).unwrap()),
                json!({"state":"done","text":"opaque answer"})
            );
            assert_eq!(
                decode(step.cancel(&call).unwrap()),
                json!({"state":"canceled"})
            );
        } else {
            assert!(submitted["error"].is_string());
        }
    }
}
