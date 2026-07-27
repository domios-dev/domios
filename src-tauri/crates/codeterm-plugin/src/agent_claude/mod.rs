pub mod provider;
pub mod usage_tui;

pub use provider::ClaudeProvider;

use crate::plugin_manager::{
    Plugin, PluginActivation, PluginCapabilities, PluginManifest, PluginMeta, PluginPermissions,
    PluginPrompt, PluginTier,
};
use crate::provider::AgentProvider;

pub const PLUGIN_ID: &str = "agent-claude";
pub const PROVIDER_ID: &str = "claude";

pub struct AgentClaudePlugin {
    activated: bool,
}

impl AgentClaudePlugin {
    pub fn new() -> Self {
        Self { activated: false }
    }

    pub fn manifest() -> PluginManifest {
        PluginManifest {
            plugin: PluginMeta {
                name: PLUGIN_ID.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Claude Code agent provider — detection, sessions, TUI control".into(),
                tier: PluginTier::Native,
                entrypoint: None,
                wasm_module: None,
                platforms: vec![],
                min_codeterm: None,
            },
            capabilities: PluginCapabilities {
                agent_providers: vec![PROVIDER_ID.into()],
                hooks: vec!["mux-state-changed".into(), "tab-title-pinned".into()],
                ..Default::default()
            },
            permissions: PluginPermissions::default(),
            activation: PluginActivation {
                triggers: vec!["on_startup".into()],
            },
            prompt: PluginPrompt::default(),
            source_path: None,
        }
    }
}

impl Plugin for AgentClaudePlugin {
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

    fn on_event(&mut self, _event: &str) {}

    fn agent_providers(&self) -> Vec<Box<dyn AgentProvider>> {
        vec![Box::new(ClaudeProvider::new())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid() {
        let m = AgentClaudePlugin::manifest();
        assert_eq!(m.plugin.name, PLUGIN_ID);
        assert_eq!(m.plugin.tier, PluginTier::Native);
        assert!(m.matches_platform());
        assert_eq!(m.capabilities.agent_providers, vec!["claude"]);
    }

    #[test]
    fn plugin_lifecycle() {
        let mut p = AgentClaudePlugin::new();
        assert!(!p.activated);
        assert!(p.activate().is_ok());
        assert!(p.activated);
        assert!(p.deactivate().is_ok());
        assert!(!p.activated);
    }

    #[test]
    fn contributes_claude_provider() {
        let p = AgentClaudePlugin::new();
        let providers = p.agent_providers();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id(), "claude");
    }

    #[test]
    fn provider_has_expected_capabilities() {
        let p = AgentClaudePlugin::new();
        let providers = p.agent_providers();
        let claude = &providers[0];
        assert_eq!(claude.display_name(), "Claude Code");
        assert_eq!(claude.compact_command(), Some("/compact"));
        assert_eq!(claude.exit_command(), Some("/exit"));
        assert!(claude.supports_lazy_skills());
        assert!(claude.has_hooks());
        assert_eq!(claude.status_bar_rows(), 3);
    }
}
