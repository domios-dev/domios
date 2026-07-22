//! `PluginManifest` — the typed shape of `plugin.json`.
//!
//! Everything a provider plugin needs to declare statically (id,
//! display name, icons, models, spawn semantics, file paths) lives
//! here as plain data. Anything that needs *logic* (parsing PTY
//! output, building shell commands from arbitrary inputs) lives in
//! `plugin.js` and gets called via `PluginVm`.
//!
//! The Rust core never hardcodes provider names — it reads manifests
//! at startup and instantiates one generic `JsProvider` per plugin.

mod capabilities;
mod config;
mod methods;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use capabilities::{Capabilities, ChatBackendCap, ChatPreprocessorCap};
pub use config::{
    CommandsConfig, CwdEncoding, IntegrationConfig, IntegrationKind, NetworkPermission,
    PermissionsConfig, RuleConfig, SerdeBusySubmit, SerdeModelInfo, SerdeReadinessStrategy,
    SerdeReasoningEffortInfo, SerdeSessionIdInjection, SerdeSystemPromptDelivery,
    SerdeTaskDelivery, SerdeVerifySubmit, SessionsConfig, SkillsConfig, SpawnConfig,
    SubprocessPermission, UsageConfig,
};
pub use methods::manifest_settings_sections;

/// Top-level `plugin.json` document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub display_name: String,
    #[serde(default)]
    pub engine: Option<String>,
    /// Minimum CodeTerm version required; gated by the plugin CLI on install/update.
    #[serde(default)]
    pub min_codeterm: Option<String>,
    #[serde(default)]
    pub data_paths: Vec<String>,
    /// Plugin/host contract version this drop-in targets. Third-party plugins
    /// must declare a value this build supports (`host_api::HOST_API_VERSION`)
    /// or they are skipped at load instead of registered against an interface
    /// they don't conform to (#16). First-party bundled plugins are exempt.
    #[serde(default)]
    pub host_api: Option<u32>,

    // relative markdown path the snapshot loads into a bounded field.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub readme: Option<String>,
    /// Relative path to an agent-facing doc (e.g. `AGENT.md`) the `get plugin`
    /// surface returns so an agent can install + self-configure the plugin
    /// (§1.7). Plain markdown; optional.
    #[serde(default)]
    pub agent_doc: Option<String>,

    #[serde(default)]
    pub icon_hint: Option<String>,
    #[serde(default)]
    pub icon_color: Option<String>,
    #[serde(default)]
    pub icon_svg: Option<String>,
    #[serde(default)]
    pub binary_name: Option<String>,
    #[serde(default)]
    pub cli_binary: Option<String>,

    // (transcriber, secret backend) leave it defaulted.
    #[serde(default)]
    pub spawn: SpawnConfig,

    // Optional features (most providers leave these omitted)
    #[serde(default)]
    pub sessions: Option<SessionsConfig>,
    #[serde(default)]
    pub integration: Option<IntegrationConfig>,
    #[serde(default)]
    pub rule: Option<RuleConfig>,
    #[serde(default)]
    pub skills: Option<SkillsConfig>,
    #[serde(default)]
    pub usage: Option<UsageConfig>,

    // Models — plugin.json uses camelCase, ModelInfo (wire format) is snake_case, so we deserialize through a local wrapper.
    #[serde(default)]
    pub models: Vec<SerdeModelInfo>,
    #[serde(default)]
    pub default_model: Option<String>,

    // Reasoning-effort levels — camelCase in plugin.json, mapped to the
    // snake_case wire `ReasoningEffortInfo` through a local wrapper.
    #[serde(default)]
    pub reasoning_efforts: Vec<SerdeReasoningEffortInfo>,
    #[serde(default)]
    pub default_reasoning_effort: Option<String>,

    // Per-platform installer (macos/linux/windows)
    #[serde(default)]
    pub installer: std::collections::HashMap<String, String>,

    #[serde(default)]
    pub commands: CommandsConfig,

    // Plugin capabilities (mostly informational; transport layer enforces permissions where it matters)
    #[serde(default)]
    pub permissions: PermissionsConfig,

    /// Which core capability traits this plugin implements. Absent → treated as
    /// an agent provider (back-compat for every existing bundled plugin, which
    /// predates this field). A plugin that declares any capability gets exactly
    /// the ones it lists.
    #[serde(default = "default_capabilities")]
    pub capabilities: Capabilities,

    /// Path (relative to the plugin dir) to a `SettingField[]` JSON schema that
    /// the Extensions UI renders as the plugin's settings panel, writing values
    /// into `config.plugins.<id>`. Optional.
    #[serde(default)]
    pub settings_schema: Option<String>,

    /// Opt-in agent-facing configuration instructions. When absent, the UI does
    /// not offer AI configuration for this plugin.
    #[serde(default)]
    pub config_help: Option<String>,

    /// The plugin's capability cannot hot-activate — it needs a CodeTerm reload
    /// to take effect (§1.8). When a plugin declaring this is *installed/enabled
    /// after daemon start*, the watcher marks it `pending_reload` instead of
    /// registering its capability live (which wouldn't work). Full daemon start
    /// activates it normally. Surfaced via `get plugin` / install `--json` and a
    /// "Reload required" banner in the Extensions UI.
    #[serde(default)]
    pub requires_reload: bool,

    // Path of plugin.js relative to the manifest, defaults to "dist/plugin.js".
    #[serde(default = "default_entry")]
    pub entry: String,

    /// Absolute path the manifest was loaded from. Filled by the loader,
    /// not present in JSON.
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

fn default_entry() -> String {
    "dist/plugin.js".to_string()
}

/// Manifests without a `capabilities` block are agent providers (every plugin
/// shipped before capabilities existed).
fn default_capabilities() -> Capabilities {
    Capabilities {
        agent_provider: true,
        secret_backend: false,
        transcriber: false,
        glance_view: false,
        status_bubble: false,
        view: false,
        chat_backend: ChatBackendCap::default(),
        chat_preprocessor: ChatPreprocessorCap::default(),
    }
}

#[cfg(test)]
mod tests;
