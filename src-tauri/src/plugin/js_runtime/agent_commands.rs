//! `JsAgentCommandHandler` — adapts a JS plugin that exports `onAgentCommand` to
//! the core [`AgentCommandProvider`](crate::agent_commands::AgentCommandProvider)
//! trait. Mirrors `JsChatBackend`/`JsTranscriber`: wraps the shared
//! `Arc<PluginVm>` and dispatches via the synchronous `call_json` (the VM is not
//! `Send`, so we call it on the caller's thread).
//!
//! Plugin surface (plan §1.10 / SDK `AgentCommandHandler`):
//! * `onAgentCommand({ sessionId, verb, args }) -> { result } | { error }`

use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;

use super::vm::{call_json, PluginVm};
use crate::agent_commands::AgentCommandProvider;

pub struct JsAgentCommandHandler {
    vm: Arc<PluginVm>,
    id: String,
    /// The `agent_commands` grant, stamped from the manifest at registration.
    granted: bool,
}

impl JsAgentCommandHandler {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>, granted: bool) -> Self {
        Self {
            vm,
            id: id.into(),
            granted,
        }
    }
}

#[derive(Deserialize)]
struct AgentCommandOut {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

impl AgentCommandProvider for JsAgentCommandHandler {
    fn id(&self) -> &str {
        &self.id
    }

    fn granted(&self) -> bool {
        self.granted
    }

    fn on_agent_command(
        &self,
        session_id: &str,
        verb: &str,
        args: &[String],
    ) -> Result<String, String> {
        let ctx = json!({ "sessionId": session_id, "verb": verb, "args": args });
        match call_json::<AgentCommandOut>(&self.vm, "onAgentCommand", &[ctx]) {
            Ok(out) => {
                if let Some(e) = out.error {
                    Err(e)
                } else {
                    Ok(out.result.unwrap_or_default())
                }
            }
            Err(e) => Err(format!("plugin onAgentCommand failed: {e:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm_with(src: &str) -> Arc<PluginVm> {
        Arc::new(PluginVm::from_source("test-agent-commands", src.to_string()).expect("vm"))
    }

    #[test]
    fn forwards_ctx_and_returns_result() {
        let vm = vm_with(
            r#"module.exports.default = {
                onAgentCommand(ctx) {
                    return { result: ctx.sessionId + "/" + ctx.verb + "/" + ctx.args.join("+") };
                }
            }"#,
        );
        let h = JsAgentCommandHandler::new(vm, "npc", true);
        let out = h
            .on_agent_command("s1", "say", &["hello".into(), "world".into()])
            .unwrap();
        assert_eq!(out, "s1/say/hello+world");
    }

    #[test]
    fn plugin_error_propagates_as_err() {
        let vm = vm_with(
            r#"module.exports.default = { onAgentCommand() { return { error: "bad verb" }; } }"#,
        );
        let h = JsAgentCommandHandler::new(vm, "npc", true);
        assert_eq!(
            h.on_agent_command("s1", "boom", &[]).unwrap_err(),
            "bad verb"
        );
    }

    #[test]
    fn missing_export_is_err() {
        let vm = vm_with(r#"module.exports.default = {}"#);
        let h = JsAgentCommandHandler::new(vm, "npc", true);
        assert!(h.on_agent_command("s1", "say", &[]).is_err());
    }

    #[test]
    fn granted_flag_is_carried() {
        let vm =
            vm_with(r#"module.exports.default = { onAgentCommand() { return { result: "" }; } }"#);
        assert!(!JsAgentCommandHandler::new(vm.clone(), "npc", false).granted());
        assert!(JsAgentCommandHandler::new(vm, "npc", true).granted());
    }
}
