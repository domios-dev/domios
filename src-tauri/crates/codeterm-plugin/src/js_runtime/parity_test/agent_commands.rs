use crate::agent_commands::{AgentCommandProvider, AgentCommandResult};

struct NativeCommandStub;

impl AgentCommandProvider for NativeCommandStub {
    fn id(&self) -> &str {
        "command-parity-fixture"
    }

    fn granted(&self) -> bool {
        true
    }

    fn on_agent_command(
        &self,
        _: &str,
        verb: &str,
        args: &[String],
        body: Option<&str>,
    ) -> Result<AgentCommandResult, String> {
        Ok(AgentCommandResult {
            result: if verb == "echo-body" {
                serde_json::json!({"args": args, "body": body}).to_string()
            } else {
                "updated".into()
            },
            catalogue_changed: verb == "refresh",
        })
    }
}

#[test]
fn native_and_js_agent_commands_report_catalogue_mutations_identically() {
    use super::super::{JsAgentCommandHandler, PluginVm};
    use std::sync::Arc;

    let source = r#"module.exports.default = {
        onAgentCommand({ verb }) {
            return { result: "updated", catalogueChanged: verb === "refresh" };
        }
    };"#;
    let vm = Arc::new(PluginVm::from_source("command-parity-fixture", source.into()).unwrap());
    let js = JsAgentCommandHandler::new(vm, "command-parity-fixture", true);
    let native = NativeCommandStub;

    assert_eq!(
        native.on_agent_command("s1", "refresh", &[], None),
        js.on_agent_command("s1", "refresh", &[], None)
    );
    assert_eq!(
        native.on_agent_command("s1", "say", &[], None),
        js.on_agent_command("s1", "say", &[], None)
    );
}

#[test]
fn native_and_js_agent_commands_preserve_body_identically() {
    use super::super::{JsAgentCommandHandler, PluginVm};
    use std::sync::Arc;

    let source = r#"module.exports.default = {
        onAgentCommand(ctx) {
            return { result: JSON.stringify({args: ctx.args, body: ctx.body === undefined ? null : ctx.body}) };
        }
    };"#;
    let vm = Arc::new(PluginVm::from_source("command-parity-fixture", source.into()).unwrap());
    let js = JsAgentCommandHandler::new(vm, "command-parity-fixture", true);
    for body in [
        None,
        Some(""),
        Some("--message"),
        Some("--"),
        Some("\r\nλ😀\n"),
    ] {
        let args = vec!["--".into(), "recipient".into()];
        assert_eq!(
            NativeCommandStub.on_agent_command("s1", "echo-body", &args, body),
            js.on_agent_command("s1", "echo-body", &args, body)
        );
    }
}

struct NativeHeadless;
impl AgentCommandProvider for NativeHeadless {
    fn id(&self) -> &str {
        "headless-parity"
    }
    fn granted(&self) -> bool {
        true
    }
    fn on_agent_command(
        &self,
        caller: &str,
        verb: &str,
        args: &[String],
        _: Option<&str>,
    ) -> Result<AgentCommandResult, String> {
        Ok(AgentCommandResult { result: serde_json::json!({"context":{"surface":"studio","instance":"logical-context","jobContext":"logical-context","tab":caller,"owner":caller,"actor":caller,"actorLabel":"agent","workspace":"workspace","team":"team","headless":true},"verb":verb,"args":args}).to_string(), catalogue_changed:false })
    }
}
#[test]
fn native_and_js_headless_dispatch_preserve_host_context_despite_forged_argument_identity() {
    use crate::{
        host_traits::PluginHostCtx,
        js_runtime::{
            host_api::PluginSandbox, host_compute::tests::AnalyticsHost,
            manifest::PermissionsConfig, JsAgentCommandHandler, PluginVm,
        },
    };
    use std::sync::Arc;
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms(
            "headless-parity",
            &PermissionsConfig {
                surfaces: true,
                agent_commands: true,
                ..Default::default()
            },
        ),
    );
    let vm = Arc::new(PluginVm::from_source_sandboxed("headless-parity", "module.exports.default={onAgentCommand(c){return host.surface.dispatch(c.verb,c.args);},onSurfaceCall(context,verb,args){return JSON.stringify({context:context,verb:verb,args:args});}}".into(),None,sandbox).unwrap());
    let host: Arc<dyn PluginHostCtx> = Arc::new(AnalyticsHost::default());
    assert!(vm.set_service(Arc::downgrade(&host)));
    let js = JsAgentCommandHandler::new(vm, "headless-parity", true);
    let args = vec![r#"{"owner":"foreign","workspace":"foreign","jobContext":"foreign"}"#.into()];
    let native = NativeHeadless
        .on_agent_command("actual-agent", "headless-probe", &args, None)
        .unwrap();
    let actual = js
        .on_agent_command("actual-agent", "headless-probe", &args, None)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual.result).unwrap(),
        serde_json::from_str::<serde_json::Value>(&native.result).unwrap()
    );
}
