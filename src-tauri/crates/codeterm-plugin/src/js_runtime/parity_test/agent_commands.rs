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
