use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationInstallOptions {
    pub consent: bool,
    #[serde(default)]
    pub precompact_memory_export: bool,
    #[serde(default)]
    pub repair: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationKind {
    ClaudeSettingsJson,
    ProviderManaged,
    PluginFile,
    None,
}

pub use crate::directory_trust::DirectoryTrustPromptResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryTrustState {
    Trusted,
    Untrusted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassifiedPromptOptionRole {
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPromptKeyStep {
    pub frame_key: String,
    pub step: Vec<u8>,
}

/// Bind address for a provider's structured-prompt server (opencode SSE bus).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredPromptServer {
    pub hostname: String,
    pub port: u16,
}

/// Whether a provider lets us choose the session id ahead of launch.
/// Drives whether CodeTerm pre-generates and tracks a UUID before spawning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionIdInjection {
    /// Provider generates its own id; CodeTerm detects it after launch.
    None,
    /// Provider takes a CLI flag for fresh sessions (e.g. claude --session-id).
    Fresh,
    /// Plus accepts explicit id when forking from --resume.
    FreshAndFork,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub session_id: String,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub file_path: String,
    /// Hook-reported transcript path (provider-owned evidence, see
    /// `crate::session_evidence`); absent ⇒ readers reconstruct from the id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_ts: Option<i64>,
}

pub fn fs_time_to_unix(t: Option<std::time::SystemTime>) -> Option<i64> {
    t.and_then(|st| st.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
    #[serde(default)]
    pub is_meta: bool,
}

// `ProviderDescriptor` is every plugin's describe() wire payload used by the
// frontend for Settings, Sidebar, and command-palette rows.

/// Severity flavor for `SettingField::Info` notes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum NoteLevel {
    Info,
    Warn,
    Error,
}

/// Visual style for status badges.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BadgeStyle {
    Green,
    Orange,
    Red,
    Gray,
}

/// Single state in a polled status badge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BadgeVariant {
    /// Value returned by the query command (e.g. "installed", "missing", "update").
    pub id: String,
    pub label: String,
    pub style: BadgeStyle,
}

/// Visual style for action buttons.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ActionStyle {
    Primary,
    Secondary,
    Danger,
}

/// What to do when the user clicks a `PathLink`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PathOpenKind {
    OpenFolder,
    OpenFile,
    RevealInExplorer,
}

mod agent_provider;
mod availability;
mod capabilities;
mod launch_permissions;
mod launch_session;
mod model_group;
mod model_info;
mod session_content;
mod settings;
mod spawn_strategy;
mod usage_snapshot;
pub use agent_provider::*;
pub use availability::*;
pub use capabilities::*;
pub use launch_permissions::*;
pub use launch_session::*;
pub use model_group::*;
pub use model_info::*;
pub use session_content::*;
pub use settings::*;
pub use spawn_strategy::*;
pub use usage_snapshot::*;

/// Wire format describing one agent provider to the frontend.
/// Built from the trait via `AgentProvider::describe()`.
///
/// `schema_version` lets us evolve the wire format without silently
/// breaking external consumers. Bump it when fields are added/removed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderDescriptor {
    pub schema_version: u32,
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_svg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,
    pub available_models: Vec<ModelInfo>,
    #[serde(default)]
    pub model_catalogue: ModelCatalogueCapability,
    /// Provider-owned groupings of `available_models` (for example the upstream
    /// company a routed model belongs to). Empty ⇒ one flat list. Core never
    /// maps a vendor id to a label; the provider declares both.
    #[serde(default)]
    pub model_groups: Vec<ModelGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// Reasoning-effort levels this provider supports at launch. Empty ⇒ the
    /// provider exposes no effort selector and callers must not offer one.
    #[serde(default)]
    pub reasoning_efforts: Vec<ReasoningEffortInfo>,
    /// Default effort id (must match one of `reasoning_efforts` when non-None).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reasoning_effort: Option<String>,
    pub settings_schema: Vec<SettingField>,
    /// Schema rendered into the global Settings → Notifications tab,
    /// next to (not inside) the per-provider settings tab. Lets a
    /// provider plug into the notifications surface without having
    /// the Notifications tab know about specific providers.
    /// Default empty — providers without notification integration
    /// (e.g. Codex today) simply contribute nothing.
    #[serde(default)]
    pub notifications_schema: Vec<SettingField>,
    pub supports_lazy_skills: bool,
    pub has_hooks: bool,
    /// Provider supplies its own explicit permission/sandbox bypass flags.
    #[serde(default)]
    pub supports_permission_bypass: bool,
    /// Optional CLI installation metadata used by install surfaces. PTY launch
    /// availability is observed from `AgentProvider::binary_name()` instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_binary: Option<String>,
    /// Shell command for installing the CLI on the current platform. Frontend
    /// pipes it into a fresh terminal tab when the user clicks Install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_command: Option<String>,
    /// Short provider-declared note picker surfaces render beside the name,
    /// e.g. a free-tier offer. Keeps the claim with the provider that owns it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// Shell command for updating the CLI on the current platform.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_command: Option<String>,
    /// How a live tab of this provider switches model; `unsupported` means the
    /// model is chosen only at spawn.
    #[serde(default)]
    pub model_switch: ModelSwitchMode,
    /// Provider-declared command that starts a fresh context in the same tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_context_command: Option<String>,
    /// Provider-declared in-agent command that compacts (summarizes) context.
    /// Consumed capability-keyed by the orchestrator refresh cycle — never a
    /// provider-id literal. Absent means no compact capability (mode-B fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_command: Option<String>,
    /// Optional `integration.trust_grant` capability. Absent ⇒ integration
    /// install needs no provider-owned trust grant step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration_trust_grant: Option<IntegrationTrustGrantCapability>,
    /// `true` when this provider declares a stop_keystroke — UI shows
    /// a Stop button on the tab that POSTs to `/api/tabs/:id/stop`.
    #[serde(default)]
    pub supports_stop: bool,
    /// `true` when the provider can render a tab as chat — either it
    /// stores sessions as tailable JSONL or serves structured chat from
    /// its own store. The UI gates the chat/terminal toggle on this
    /// instead of a hardcoded provider list.
    #[serde(default)]
    pub supports_chat_ui: bool,
    /// `true` when the provider exposes a structured prompt source in its
    /// current interactive run mode (claude AskUserQuestion in the JSONL
    /// transcript, opencode SSE bus). Core builds `pending_prompt` from the
    /// structured handle first and only falls back to PTY scraping otherwise.
    #[serde(default)]
    pub supports_structured_prompts: bool,
    /// `true` when the provider can recognize its own permission prompts.
    #[serde(default)]
    pub supports_permission_auto_confirm: bool,
    /// `true` when a chatBackend plugin implements watcher mode (manifest
    /// `chatBackend: { watcher: true }`). Always `false` for PTY transports.
    #[serde(default)]
    pub supports_watcher: bool,
    /// Maximum text body size accepted as an inline paste, when declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paste_limit_bytes: Option<usize>,
    /// Trust status of the bundle backing this provider. `system` =
    /// shipped by CodeTerm, hash matched the build-time table, or was
    /// installed from the official codeterm-core system channel.
    /// `unverified` = same id as a system plugin but the bundle on
    /// disk has a different hash (user-modified or tampered).
    /// `third_party` = plugin id not in the system table.
    /// `unknown` = native (Rust-side) provider, no hash to check.
    #[serde(default)]
    pub trust: ProviderTrust,
    /// Runtime backing this provider — the spawn discriminant (was the `kind` string).
    pub transport: ProviderTransport,
    /// Behavior surface the provider exposes for configuration.
    #[serde(default)]
    pub behavior_config: BehaviorConfig,
    /// Named presets the provider offers (chat backends); agents default empty.
    #[serde(default)]
    pub presets: Vec<crate::chatbackend_provider::PresetInfo>,
    /// How a system prompt reaches the provider.
    pub system_prompt_delivery: SystemPromptDelivery,
    /// Who composes this provider's conversational context. Skipped on the wire
    /// while `Plugin`, so a provider that never declares it serializes exactly
    /// as it did before the field existed.
    #[serde(default, skip_serializing_if = "ContextOwner::is_plugin")]
    pub context_owner: ContextOwner,
    /// What enumeration observed about `binary_name()` on this machine.
    /// Defaults to `unknown` so a descriptor nobody probed cannot claim more
    /// than it knows; enumeration overwrites it with the real observation.
    #[serde(default)]
    pub availability: ProviderAvailability,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderUpdateAvailability {
    pub current_version: String,
    pub latest_version: String,
}

/// Current `ProviderDescriptor` schema version. Bump on breaking change.
pub const PROVIDER_DESCRIPTOR_SCHEMA_VERSION: u32 = 17;

/// One provider-owned usage log to ingest for a session. `session_id` scopes a
/// log shared by many sessions (e.g. one sqlite database); a per-session file
/// leaves it `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSourceSpec {
    pub path: PathBuf,
    pub session_id: Option<String>,
}

impl UsageSourceSpec {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            session_id: None,
        }
    }
}

/// The default usage-source set: the session file plus the provider's own extra
/// logs beside it. Shared by the trait default and the JS bridge so a plugin that
/// exports nothing keeps the native answer.
pub fn usage_sources_from_session_file<P: AgentProvider + ?Sized>(
    provider: &P,
    cwd: &str,
    session_id: &str,
) -> Vec<UsageSourceSpec> {
    let Some(main) = provider.session_file_path(cwd, session_id) else {
        return Vec::new();
    };
    std::iter::once(UsageSourceSpec::file(main.clone()))
        .chain(
            provider
                .additional_usage_sources(&main, session_id)
                .into_iter()
                .map(UsageSourceSpec::file),
        )
        .collect()
}
