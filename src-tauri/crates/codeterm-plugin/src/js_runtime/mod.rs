mod agent_commands;
mod bin_resolve;
mod bubble;
#[cfg(test)]
mod call_gate;
mod chatbackend;
mod chatpreprocessor;
mod glance;
pub mod grants;
mod host_api;
mod host_compute;
mod host_credentials;
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
pub use bin_resolve::{
    forget_resolved_bin, forget_resolved_bins, probe_bin, probe_bin_in, probe_bin_with_environment,
    resolved_bin_path, well_known_bin_dirs, BinProbe, BinResolution, BinSearchEnvironment,
    EnvironmentValue,
};
pub use bubble::JsBubbleProvider;
pub use chatbackend::JsChatBackend;
pub use chatpreprocessor::JsChatPreprocessor;
pub use glance::JsGlanceProvider;
pub use transcriber::JsTranscriber;
pub use view::JsViewProvider;

/// Serializes the tests that mutate process-wide env or the shared resolver
/// memo, which now live in more than one module.
#[cfg(test)]
pub(crate) static PROCESS_TEST_LOCK: std::sync::LazyLock<parking_lot::Mutex<()>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(()));

#[cfg(test)]
mod parity_test;

pub use hot_reload::{
    pending_reload_ids, set_watcher_service, CapabilityRegistries, HotReloadRegistry,
};
pub use loader::{default_roots, load_one, scan_roots, LoadOutcome};
pub use provider::JsProvider;

pub use manifest::{
    manifest_settings_sections, resolve_hook_event_roster, substitute_template,
    unknown_template_placeholder, CommandsConfig, CredentialConfig, CwdEncoding, IntegrationConfig,
    IntegrationKind, PermissionsConfig, PluginManifest, RuleConfig, SerdeHookEventBinding,
    SerdeModelInfo, SerdeProviderReadinessStrategy, SerdeReadinessStrategy,
    SerdeSessionIdInjection, SerdeTaskDelivery, SerdeVerifySubmit, SessionsConfig, SkillsConfig,
    SpawnConfig, TrustGrantConfig, UsageConfig, LAUNCH_TEMPLATE_PLACEHOLDERS,
    RESUME_TEMPLATE_PLACEHOLDERS,
};
pub use vm::{call_json, has_method, PluginVm, VmError, ASYNC_JOB_TIMEOUT_ERROR};
