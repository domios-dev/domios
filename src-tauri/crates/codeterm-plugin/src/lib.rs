pub mod activity;
pub mod agent_claude;
pub mod agent_commands;
pub mod audit;
pub mod bubble;
pub mod bundle_hash;
pub mod capability;
pub mod chatbackend_provider;
pub mod chatpreprocessor;
pub mod common;
pub mod detector;
pub mod embedded;
pub mod glance;
pub mod host_traits;
pub mod js_runtime;
pub mod jsonl_window;
pub mod model_catalog;
pub mod pane_state;
pub mod plugin_manager;
pub mod prompt;
pub mod prompts;
pub mod provider;
pub mod rich_to_chat;
pub mod structured_prompt;
pub mod system_plugins;
pub mod transcribe;
pub mod usage_parser;
pub mod view;
pub mod view_host;

pub use capability::CapabilityRegistry;
pub use host_traits::{PluginHostCtx, PluginProviderRegistrar, SecretBackendRegistrar};
pub use js_runtime::hot_reload::{set_watcher_service, CapabilityRegistries, HotReloadRegistry};
pub use plugin_manager::{
    Plugin, PluginActivation, PluginCapabilities, PluginManager, PluginManifest, PluginMeta,
    PluginPermissions, PluginPrompt, PluginTier,
};
pub use provider::AgentProvider;
