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
        _: &[String],
    ) -> Result<AgentCommandResult, String> {
        Ok(AgentCommandResult {
            result: "updated".into(),
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
        native.on_agent_command("s1", "refresh", &[]),
        js.on_agent_command("s1", "refresh", &[])
    );
    assert_eq!(
        native.on_agent_command("s1", "say", &[]),
        js.on_agent_command("s1", "say", &[])
    );
}
