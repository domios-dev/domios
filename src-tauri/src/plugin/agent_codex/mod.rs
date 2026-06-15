//! agent-codex — bundled native plugin that provides the OpenAI Codex agent.
//!
//! Owns `CodexProvider` and registers it as an `AgentProvider`
//! during activation. All Codex-specific knowledge (detection patterns,
//! session file layout, TUI noise, slash commands) lives in this module —
//! core never references Codex directly.

pub mod integration;
pub mod provider;
pub mod status_tui;
pub mod usage;

pub use provider::CodexProvider;

use super::Plugin;
use crate::agent::provider::AgentProvider;
use crate::api::events::SystemEvent;

pub const PLUGIN_ID: &str = "agent-codex";
pub const PROVIDER_ID: &str = "codex";

/// Bundled native plugin that contributes the OpenAI Codex agent provider.
pub struct AgentCodexPlugin {
    activated: bool,
}

impl AgentCodexPlugin {
    pub fn new() -> Self {
        Self { activated: false }
    }

    pub fn manifest() -> super::PluginManifest {
        super::PluginManifest {
            plugin: super::PluginMeta {
                name: PLUGIN_ID.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "OpenAI Codex agent provider — detection, sessions, TUI control"
                    .into(),
                tier: super::PluginTier::Native,
                entrypoint: None,
                wasm_module: None,
                platforms: vec![],
                min_codeterm: None,
            },
            capabilities: super::PluginCapabilities {
                agent_providers: vec![PROVIDER_ID.into()],
                hooks: vec!["mux-state-changed".into(), "pane-title-pinned".into()],
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

impl Plugin for AgentCodexPlugin {
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
        vec![Box::new(CodexProvider::new())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid() {
        let m = AgentCodexPlugin::manifest();
        assert_eq!(m.plugin.name, PLUGIN_ID);
        assert_eq!(m.plugin.tier, super::super::PluginTier::Native);
        assert!(m.matches_platform());
        assert_eq!(m.capabilities.agent_providers, vec!["codex"]);
    }

    #[test]
    fn plugin_lifecycle() {
        let mut p = AgentCodexPlugin::new();
        assert!(!p.activated);
        assert!(p.activate().is_ok());
        assert!(p.activated);
        assert!(p.deactivate().is_ok());
        assert!(!p.activated);
    }

    #[test]
    fn contributes_codex_provider() {
        let p = AgentCodexPlugin::new();
        let providers = p.agent_providers();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id(), "codex");
    }

    #[test]
    fn provider_has_expected_capabilities() {
        let p = AgentCodexPlugin::new();
        let providers = p.agent_providers();
        let codex = &providers[0];
        assert_eq!(codex.display_name(), "Codex");
        assert_eq!(codex.compact_command(), Some("/compact"));
        assert_eq!(codex.exit_command(), Some("/exit"));
        assert!(!codex.supports_lazy_skills());
        assert!(codex.has_hooks());
        assert_eq!(codex.status_bar_rows(), 1);
    }
}
