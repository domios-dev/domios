//! `JsAgentCommandHandler` — adapts a JS `onAgentCommand` export to the core
//! [`AgentCommandProvider`](crate::agent_commands::AgentCommandProvider) trait via
//! synchronous `call_json`. Surface: `onAgentCommand({ sessionId, verb, args, body? }) -> { result } | { error }`.

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
        body: Option<&str>,
    ) -> Result<crate::agent_commands::AgentCommandResult, String> {
        let mut ctx = json!({ "sessionId": session_id, "verb": verb, "args": args });
        if let Some(body) = body {
            ctx["body"] = body.into();
        }
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
mod tests;
