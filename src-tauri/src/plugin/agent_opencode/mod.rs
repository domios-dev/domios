//! agent-opencode — bundled native plugin that provides the OpenCode agent.
//!
//! Owns `OpenCodeProvider` and registers it as an `AgentProvider`
//! during activation. All OpenCode-specific knowledge (detection patterns,
//! session file layout, TUI noise, slash commands) lives in this module —
//! core never references OpenCode directly.

pub mod hooks;
pub mod provider;

pub use provider::OpenCodeProvider;

use crate::agent::provider::AgentProvider;
use crate::api::events::SystemEvent;
use super::Plugin;

pub const PLUGIN_ID: &str = "agent-opencode";
pub const PROVIDER_ID: &str = "opencode";

/// Bundled native plugin that contributes the OpenCode agent provider.
pub struct AgentOpenCodePlugin {
    activated: bool,
}

impl AgentOpenCodePlugin {
    pub fn new() -> Self {
        Self { activated: false }
    }

    pub fn manifest() -> super::PluginManifest {
        super::PluginManifest {
            plugin: super::PluginMeta {
                name: PLUGIN_ID.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "OpenCode agent provider — detection, sessions, TUI control"
                    .into(),
                tier: super::PluginTier::Native,
                entrypoint: None,
                wasm_module: None,
                platforms: vec![],
                min_codeterm: None,
            },
            capabilities: super::PluginCapabilities {
                agent_providers: vec![PROVIDER_ID.into()],
                hooks: vec![
                    "mux-state-changed".into(),
                    "pane-title-pinned".into(),
                ],
                ..Default::default()
            },
            permissions: super::PluginPermissions::default(),
            activation: super::PluginActivation {
                triggers: vec!["on_startup".into()],
            },
            prompt: super::PluginPrompt::default(),
            source_path: None,
        }
    }
}

impl Plugin for AgentOpenCodePlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn activate(&mut self) -> Result<(), String> {
        log::info!("[{}] activating", PLUGIN_ID);
        self.activated = true;
        Ok(())
    }

    fn deactivate(&mut self) -> Result<(), String> {
        log::info!("[{}] deactivating", PLUGIN_ID);
        self.activated = false;
        Ok(())
    }

    fn on_event(&mut self, _event: &SystemEvent) {}

    fn agent_providers(&self) -> Vec<Box<dyn AgentProvider>> {
        vec![Box::new(OpenCodeProvider::new())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid() {
        let m = AgentOpenCodePlugin::manifest();
        assert_eq!(m.plugin.name, PLUGIN_ID);
        assert_eq!(m.plugin.tier, super::super::PluginTier::Native);
        assert!(m.matches_platform());
        assert_eq!(m.capabilities.agent_providers, vec!["opencode"]);
    }

    #[test]
    fn plugin_lifecycle() {
        let mut p = AgentOpenCodePlugin::new();
        assert!(!p.activated);
        assert!(p.activate().is_ok());
        assert!(p.activated);
        assert!(p.deactivate().is_ok());
        assert!(!p.activated);
    }

    #[test]
    fn contributes_opencode_provider() {
        let p = AgentOpenCodePlugin::new();
        let providers = p.agent_providers();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id(), "opencode");
    }

    #[test]
    fn provider_has_expected_capabilities() {
        let p = AgentOpenCodePlugin::new();
        let providers = p.agent_providers();
        let oc = &providers[0];
        assert_eq!(oc.display_name(), "OpenCode");
        assert_eq!(oc.compact_command(), Some("/compact"));
        assert_eq!(oc.exit_command(), Some("/exit"));
        assert!(!oc.supports_lazy_skills());
        assert!(!oc.has_hooks());
    }
}
