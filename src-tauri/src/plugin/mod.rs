pub mod activity;

pub use codeterm_plugin::agent_claude;
pub use codeterm_plugin::agent_commands;
pub use codeterm_plugin::audit;
pub use codeterm_plugin::bubble;
pub use codeterm_plugin::bundle_hash;
pub use codeterm_plugin::capability;
pub use codeterm_plugin::chatbackend_provider;
pub use codeterm_plugin::chatpreprocessor;
pub use codeterm_plugin::common;
pub use codeterm_plugin::detector;
pub use codeterm_plugin::embedded;
pub use codeterm_plugin::glance;
pub use codeterm_plugin::host_traits;
pub use codeterm_plugin::js_runtime;
pub use codeterm_plugin::model_catalog;
pub use codeterm_plugin::plugin_manager;
pub use codeterm_plugin::prompt;
pub use codeterm_plugin::provider;
pub use codeterm_plugin::rich_to_chat;
pub use codeterm_plugin::structured_prompt;
pub use codeterm_plugin::system_plugins;
pub use codeterm_plugin::tab_state;
pub use codeterm_plugin::transcribe;
pub use codeterm_plugin::usage_parser;
pub use codeterm_plugin::view;
pub use codeterm_plugin::view_host;
pub use codeterm_plugin::webhook;

pub use codeterm_plugin::plugin_manager::SkillFile;
pub use codeterm_plugin::AgentProvider;
pub use codeterm_plugin::{set_watcher_service, CapabilityRegistries, HotReloadRegistry};
pub use codeterm_plugin::{
    CapabilityRegistry, PluginHostCtx, PluginProviderRegistrar, SecretBackendRegistrar,
};
pub use codeterm_plugin::{
    Plugin, PluginActivation, PluginCapabilities, PluginManager, PluginManifest, PluginMeta,
    PluginPermissions, PluginPrompt, PluginTier,
};
pub use codeterm_plugin::{
    WebhookDelivery, WebhookReceiveContext, WebhookReceiveError, WebhookReceiver,
    WebhookReceiverKind, WebhookRegistry, WebhookRegistryHandle,
};
