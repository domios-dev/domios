pub mod activity;
pub mod agent_claude;
pub mod agent_commands;
pub mod agent_prompt_lines;
pub mod agent_runtime;
pub mod agent_task;
pub mod agent_verbs;
pub mod audit;
pub mod bubble;
pub mod bundle_hash;
#[cfg(any(test, feature = "test-support"))]
mod bundle_test_support;
pub mod bundled;
pub mod capability;
pub mod chatbackend_provider;
pub mod chatpreprocessor;
pub mod common;
pub mod converter;
pub mod detector;
pub mod directory_trust;
pub mod embedded;
pub mod event_source;
pub mod glance;
pub mod global_writes;
pub mod graph_events;
pub mod graph_nodes;
pub mod host_traits;
pub mod js_runtime;
pub(crate) mod json_boolean_record;
pub mod jsonl_window;
pub mod local_recording;
pub mod model_catalog;
pub mod model_providers;
pub mod permission_prompt;
pub mod plugin_icon;
pub mod plugin_lifecycle;
pub mod plugin_manager;
pub mod prompt;
pub mod prompts;
pub mod provider;
pub mod provider_cli;
pub mod provider_command;
pub mod provider_compat;
pub mod provider_version;
pub mod rich_to_chat;
pub mod session_evidence;
pub mod structured_prompt;
pub mod surfaces;
pub mod system_plugins;
pub mod tab_menu;
pub mod tab_state;
pub(crate) mod toml_folder_trust;
pub mod transcribe;
pub mod update_prompt;
pub mod usage_parser;
pub mod view;
pub mod view_host;
pub mod webhook;

pub use capability::CapabilityRegistry;
pub use event_source::EventSourceRegistry;
pub use graph_nodes::GraphNodeRegistry;
pub use host_traits::{PluginHostCtx, PluginProviderRegistrar, SecretBackendRegistrar};
pub use js_runtime::hot_reload::{set_watcher_service, CapabilityRegistries, HotReloadRegistry};
pub use js_runtime::manifest::WebhookReceiverKind;
pub use plugin_manager::{
    Plugin, PluginActivation, PluginCapabilities, PluginManager, PluginManifest, PluginMeta,
    PluginPermissions, PluginPrompt, PluginTier,
};
pub use provider::AgentProvider;
pub use webhook::{
    WebhookAudio, WebhookDelivery, WebhookReceiveContext, WebhookReceiveError, WebhookReceiver,
    WebhookRegistry, WebhookRegistryHandle,
};

#[cfg(test)]
mod test_support;
