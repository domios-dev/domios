mod agent_commands;
mod bubble;
mod chatbackend;
mod chatpreprocessor;
mod glance;
pub mod grants;
mod host_api;
mod host_compute;
mod host_exec;
mod host_net;
pub mod hot_reload;
pub mod loader;
pub mod manifest;
pub mod plugin_config;
mod provider;
pub(crate) mod toolcall_parse;
mod transcriber;
mod view;
mod vm;

pub use agent_commands::JsAgentCommandHandler;
pub use bubble::JsBubbleProvider;
pub use chatbackend::JsChatBackend;
pub use chatpreprocessor::JsChatPreprocessor;
pub use glance::JsGlanceProvider;
pub use transcriber::JsTranscriber;
pub use view::JsViewProvider;

#[cfg(test)]
mod parity_test;

pub use hot_reload::{
    pending_reload_ids, set_watcher_service, CapabilityRegistries, HotReloadRegistry,
};
pub use loader::{default_roots, load_one, scan_roots, LoadOutcome};
pub use provider::JsProvider;

pub use manifest::{
    manifest_settings_sections, resolve_hook_event_roster, CommandsConfig, CwdEncoding,
    IntegrationConfig, IntegrationKind, PermissionsConfig, PluginManifest, RuleConfig,
    SerdeHookEventBinding, SerdeModelInfo, SerdeProviderReadinessStrategy, SerdeReadinessStrategy,
    SerdeSessionIdInjection, SerdeTaskDelivery, SerdeVerifySubmit, SessionsConfig, SkillsConfig,
    SpawnConfig, UsageConfig,
};
pub use vm::{call_json, has_method, PluginVm, VmError};
