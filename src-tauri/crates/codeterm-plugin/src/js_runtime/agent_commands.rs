//! `JsAgentCommandHandler` — adapts a JS `onAgentCommand` export to the core
//! [`AgentCommandProvider`](crate::agent_commands::AgentCommandProvider) trait via
//! synchronous `call_json`. Surface: `onAgentCommand({ sessionId, verb, args }) -> { result } | { error }`.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;

use super::vm::{call_json, PluginVm};
use crate::agent_commands::AgentCommandProvider;
use crate::agent_verbs::AgentVerbCatalog;

pub struct JsAgentCommandHandler {
    vm: Arc<PluginVm>,
    id: String,
    /// The `agent_commands` grant, stamped from the manifest at registration.
    granted: bool,
    catalog: Option<AgentVerbCatalog>,
}

impl JsAgentCommandHandler {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>, granted: bool) -> Self {
        Self {
            vm,
            id: id.into(),
            granted,
            catalog: None,
        }
    }

    pub fn with_catalog(mut self, catalog: AgentVerbCatalog) -> Self {
        self.catalog = Some(catalog);
        self
    }
}

pub fn catalog_from_manifest(manifest: &super::PluginManifest, core: bool) -> AgentVerbCatalog {
    AgentVerbCatalog::compose(
        &manifest.id,
        &manifest.display_name,
        manifest.description.as_deref(),
        &manifest.agent_verbs,
        manifest.config_help.as_deref(),
        &manifest.agent_keywords,
    )
    .with_guide(agent_guide_from_manifest(manifest))
    .with_prompt_line(core, manifest.agent_prompt_line.clone())
}

fn agent_guide_from_manifest(manifest: &super::PluginManifest) -> Option<String> {
    let relative = manifest.agent_guide.as_deref()?;
    let bundle_dir = manifest.source_path.as_deref()?.parent()?;
    match codeterm_util::plugin_agent_guide::load_agent_guide(bundle_dir, relative) {
        Ok(guide) => Some(guide.text),
        Err(error) => {
            log::warn!("[plugin] {}: {error}", manifest.id);
            None
        }
    }
}

#[derive(Deserialize)]
struct AgentCommandOut {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default, rename = "catalogueChanged")]
    catalogue_changed: bool,
}

impl AgentCommandProvider for JsAgentCommandHandler {
    fn id(&self) -> &str {
        &self.id
    }

    fn granted(&self) -> bool {
        self.granted
    }

    fn catalog(&self) -> Option<AgentVerbCatalog> {
        self.catalog.clone()
    }

    fn on_agent_command(
        &self,
        session_id: &str,
        verb: &str,
        args: &[String],
    ) -> Result<crate::agent_commands::AgentCommandResult, String> {
        let ctx = json!({ "sessionId": session_id, "verb": verb, "args": args });
        let _caller = super::host_surface::AgentCallerScope::enter(session_id);
        match call_json::<AgentCommandOut>(&self.vm, "onAgentCommand", &[ctx]) {
            Ok(out) => {
                if let Some(e) = out.error {
                    Err(e)
                } else {
                    Ok(crate::agent_commands::AgentCommandResult {
                        result: out.result.unwrap_or_default(),
                        catalogue_changed: out.catalogue_changed,
                    })
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
        assert_eq!(out.result, "s1/say/hello+world");
        assert!(!out.catalogue_changed);
    }

    #[test]
    fn catalogue_changed_flag_is_returned_from_agent_command() {
        let vm = vm_with(
            r#"module.exports.default = {
                onAgentCommand() { return { result: "updated", catalogueChanged: true }; }
            }"#,
        );
        let h = JsAgentCommandHandler::new(vm, "provider-plugin", true);

        let out = h.on_agent_command("s1", "refresh", &[]).unwrap();

        assert_eq!(out.result, "updated");
        assert!(out.catalogue_changed);
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
