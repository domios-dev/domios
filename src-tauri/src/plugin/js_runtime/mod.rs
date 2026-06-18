//! Embedded JavaScript runtime that hosts plugin code on the Rust side.
//!
//! Provider plugins ship as `plugin.js` bundles (esbuild output).
//! Headless / background Rust code paths that need provider behaviour
//! (detection, parsing, status mapping, spawn-string building) call
//! into the bundle via `PluginVm` — a per-plugin QuickJS context.
//!
//! The WebView side (chat renderer, pane chrome actions) loads the
//! same bundle separately via dynamic import; this module is strictly
//! the Rust-side runtime.
//!
//! ## Threading
//!
//! QuickJS contexts are not `Sync` — concurrent access from multiple
//! threads would corrupt the heap. `PluginVm` owns its context behind
//! a `parking_lot::Mutex` so all calls serialize. Hot-path methods
//! (`detect_from_output` runs on every PTY chunk) are still cheap
//! because QuickJS evaluations are sub-millisecond for the kind of
//! string work plugins do.

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
mod host_secret;
pub mod hot_reload;
pub mod loader;
pub mod manifest;
pub mod plugin_config;
mod provider;
mod secret_backend;
mod transcriber;
mod view;
mod vm;

pub use agent_commands::JsAgentCommandHandler;
pub use bubble::JsBubbleProvider;
pub use chatbackend::JsChatBackend;
pub use chatpreprocessor::JsChatPreprocessor;
pub use glance::JsGlanceProvider;
pub use secret_backend::JsSecretBackend;
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
    manifest_settings_sections, CommandsConfig, CwdEncoding, IntegrationConfig, IntegrationKind,
    PermissionsConfig, PluginManifest, RuleConfig, SerdeModelInfo, SerdeReadinessStrategy,
    SerdeSessionIdInjection, SerdeTaskDelivery, SerdeVerifySubmit, SessionsConfig, SkillsConfig,
    SpawnConfig, UsageConfig,
};
pub use vm::{call_json, PluginVm, VmError};
