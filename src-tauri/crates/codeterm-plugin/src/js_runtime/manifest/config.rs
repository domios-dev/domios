use serde::{Deserialize, Serialize};

use crate::provider::{
    BusySubmitStrategy, HookEventBinding, HookIngestionRoute, ModelAvailability, ModelInfo,
    ProviderReadinessStrategy, ProviderStateVerdict, ReadinessStrategy, ReasoningEffortInfo,
    SessionIdInjection, SystemPromptDelivery, TaskDelivery, VerifySubmitStrategy,
};

/// Hosts/origins a plugin may reach via `host.fetch`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetworkPermission {
    #[serde(default)]
    pub allow: Vec<String>,
}

/// Binaries a plugin may run via `host.exec`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SubprocessPermission {
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SpawnConfig {
    pub readiness_strategy: SerdeReadinessStrategy,
    pub task_delivery: SerdeTaskDelivery,
    #[serde(default)]
    pub system_prompt_delivery: SerdeSystemPromptDelivery,
    #[serde(default)]
    pub provider_readiness: SerdeProviderReadinessStrategy,
    pub verify_submit: SerdeVerifySubmit,
    #[serde(default)]
    pub busy_submit: SerdeBusySubmit,
    #[serde(default)]
    pub session_id_injection: SerdeSessionIdInjection,
    #[serde(default)]
    pub status_bar_rows: u16,
    #[serde(default)]
    pub agent_state_status_reconciliation: bool,
    #[serde(default)]
    pub supports_lazy_skills: bool,
    #[serde(default)]
    pub has_hooks: bool,
    /// Declared hook event roster (event name → core ingestion route).
    /// Absent ⇒ default roster: native fallback's declaration, else SessionStart-only when hooked.
    #[serde(default)]
    pub hook_event_roster: Option<Vec<SerdeHookEventBinding>>,
    /// Plugin exports `parseInteractiveEvents` — host calls it to extract
    /// interactive prompts. When false the native fallback handles it.
    #[serde(default)]
    pub interactive_events: bool,
    /// Provider exposes a structured prompt source in its current run mode
    /// (e.g. opencode's local SSE event bus). When true the host subscribes
    /// to the structured handle and builds `pending_prompt` from it instead
    /// of scraping the PTY picker.
    #[serde(default)]
    pub supports_structured_prompts: bool,
    /// Provider's structured handle is a local SERVER it must bind — the host
    /// allocates a fixed `127.0.0.1:port` and passes it as
    /// `structuredPromptServer` at launch (opencode `--hostname/--port`).
    /// Pull-handle providers (claude reads its own JSONL) leave this false.
    #[serde(default)]
    pub structured_prompt_server: bool,
    /// TUI command that prints the account usage screen (e.g. claude `/usage`,
    /// codex `/status`). Pairs with the plugin's `parseUsage` export; the
    /// usage poller skips providers without both.
    #[serde(default)]
    pub usage_probe_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SerdeSystemPromptDelivery {
    #[default]
    External,
    CliFlag {
        flag: String,
    },
    SessionConfig,
}

impl From<&SerdeSystemPromptDelivery> for SystemPromptDelivery {
    fn from(value: &SerdeSystemPromptDelivery) -> Self {
        match value {
            SerdeSystemPromptDelivery::External => Self::External,
            SerdeSystemPromptDelivery::CliFlag { flag } => Self::CliFlag(flag.clone()),
            SerdeSystemPromptDelivery::SessionConfig => Self::SessionConfig,
        }
    }
}

/// Mirror of `crate::provider::ReadinessStrategy` with snake_case
/// serde shape suitable for plugin.json authors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[allow(clippy::enum_variant_names)]
pub enum SerdeReadinessStrategy {
    PollShellPrompt { timeout_secs: u64 },
    FixedDelay { duration_ms: u64 },
}

impl Default for SerdeReadinessStrategy {
    fn default() -> Self {
        Self::PollShellPrompt { timeout_secs: 30 }
    }
}

impl From<&SerdeReadinessStrategy> for ReadinessStrategy {
    fn from(s: &SerdeReadinessStrategy) -> Self {
        match s {
            SerdeReadinessStrategy::PollShellPrompt { timeout_secs } => Self::PollShellPrompt {
                timeout: std::time::Duration::from_secs(*timeout_secs),
            },
            SerdeReadinessStrategy::FixedDelay { duration_ms } => Self::FixedDelay {
                duration: std::time::Duration::from_millis(*duration_ms),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SerdeProviderReadinessStrategy {
    TuiFrame { timeout_secs: u64 },
    StructuralComposer { timeout_secs: u64 },
}

impl Default for SerdeProviderReadinessStrategy {
    fn default() -> Self {
        Self::TuiFrame { timeout_secs: 30 }
    }
}

impl From<&SerdeProviderReadinessStrategy> for ProviderReadinessStrategy {
    fn from(strategy: &SerdeProviderReadinessStrategy) -> Self {
        match strategy {
            SerdeProviderReadinessStrategy::TuiFrame { timeout_secs } => Self::TuiFrame {
                timeout: std::time::Duration::from_secs(*timeout_secs),
            },
            SerdeProviderReadinessStrategy::StructuralComposer { timeout_secs } => {
                Self::StructuralComposer {
                    timeout: std::time::Duration::from_secs(*timeout_secs),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerdeTaskDelivery {
    InlineFlag,
    PtyInput,
}

impl Default for SerdeTaskDelivery {
    fn default() -> Self {
        Self::PtyInput
    }
}

impl From<SerdeTaskDelivery> for TaskDelivery {
    fn from(s: SerdeTaskDelivery) -> Self {
        match s {
            SerdeTaskDelivery::InlineFlag => Self::InlineFlag,
            SerdeTaskDelivery::PtyInput => Self::PtyInput,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[allow(clippy::enum_variant_names)]
pub enum SerdeVerifySubmit {
    ShellPrompt {
        timeout_secs: u64,
    },
    TuiInputDrained {
        #[serde(default)]
        initial_key_delay_ms: u64,
    },
    None,
}

impl Default for SerdeVerifySubmit {
    fn default() -> Self {
        Self::ShellPrompt { timeout_secs: 5 }
    }
}

impl From<&SerdeVerifySubmit> for VerifySubmitStrategy {
    fn from(s: &SerdeVerifySubmit) -> Self {
        match s {
            SerdeVerifySubmit::ShellPrompt { timeout_secs } => Self::ShellPrompt {
                timeout: std::time::Duration::from_secs(*timeout_secs),
            },
            SerdeVerifySubmit::TuiInputDrained {
                initial_key_delay_ms,
            } => Self::TuiInputDrained {
                initial_key_delay: std::time::Duration::from_millis(*initial_key_delay_ms),
            },
            SerdeVerifySubmit::None => Self::None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SerdeBusySubmit {
    #[default]
    SubmitNormally,
    Key {
        key: String,
    },
}

impl From<&SerdeBusySubmit> for BusySubmitStrategy {
    fn from(value: &SerdeBusySubmit) -> Self {
        match value {
            SerdeBusySubmit::SubmitNormally => Self::SubmitNormally,
            SerdeBusySubmit::Key { key } => Self::Key(key.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SerdeSessionIdInjection {
    #[default]
    None,
    Fresh,
    FreshAndFork,
}

impl From<SerdeSessionIdInjection> for SessionIdInjection {
    fn from(s: SerdeSessionIdInjection) -> Self {
        match s {
            SerdeSessionIdInjection::None => Self::None,
            SerdeSessionIdInjection::Fresh => Self::Fresh,
            SerdeSessionIdInjection::FreshAndFork => Self::FreshAndFork,
        }
    }
}

/// One declared hook event in `spawn.hookEventRoster`. `route` resolves
/// against the closed core ingestion set; unknown names fail closed at install.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SerdeHookEventBinding {
    pub event: String,
    pub route: String,
    /// Only meaningful on the `session_state` route; unknown names fail closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
}

/// Resolve a manifest-declared roster against the closed ingestion-route set.
/// Any unknown route fails the whole roster — never a silent partial drop.
pub fn resolve_hook_event_roster(
    entries: &[SerdeHookEventBinding],
) -> Result<Vec<HookEventBinding>, String> {
    entries
        .iter()
        .map(|entry| {
            let route = HookIngestionRoute::parse(&entry.route).ok_or_else(|| {
                format!(
                    "hook event '{}' declares unknown ingestion route '{}'",
                    entry.event, entry.route
                )
            })?;
            let verdict = match entry.verdict.as_deref() {
                None => None,
                Some(name) => Some(ProviderStateVerdict::parse(name).ok_or_else(|| {
                    format!(
                        "hook event '{}' declares unknown state verdict '{}'",
                        entry.event, name
                    )
                })?),
            };
            Ok(HookEventBinding {
                event: entry.event.clone(),
                route,
                verdict,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SessionsConfig {
    /// Filesystem root the provider writes session files into. May
    /// start with `~/` (expanded to home dir). E.g.
    /// `~/.claude/projects`.
    pub root_dir: String,
    /// How a working-directory path is mapped into a subdirectory name
    /// under `root_dir`. Claude encodes `/Users/foo/code` as
    /// `-Users-foo-code`.
    #[serde(default)]
    pub cwd_encoding: CwdEncoding,
    /// File extension session files use (`jsonl`, `json`, etc).
    #[serde(default = "default_session_ext")]
    pub file_extension: String,
}

fn default_session_ext() -> String {
    "jsonl".to_string()
}

/// A credential file the HOST reads on the plugin's behalf. Declaring one
/// REVOKES the plugin's own read access to that file, even inside a read root:
/// public fields come back through `host.credentialPublic`, secret fields are
/// only ever substituted into an outbound `host.fetch` header by the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CredentialConfig {
    /// Name the plugin uses to reference this credential.
    pub id: String,
    /// JSON file holding it. May start with `~/`.
    pub file: String,
    /// Non-secret fields the plugin may read: name -> dotted path in the file.
    #[serde(default)]
    pub public: std::collections::BTreeMap<String, String>,
    /// Secret fields: name -> dotted path. Never returned to the plugin.
    #[serde(default)]
    pub secret: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CwdEncoding {
    /// Slashes (and backslashes) become dashes, leading separator
    /// kept: `/Users/foo/code` → `-Users-foo-code`.
    #[default]
    DashEncoded,
    /// Path used as-is (rare).
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IntegrationConfig {
    /// What kind of integration to install. The kind declares whether native
    /// or provider-managed review/backup/restore semantics apply.
    pub kind: IntegrationKind,
    /// Where the integration is stored (e.g. `~/.claude/settings.json`).
    pub settings_path: String,
    /// Optional `integration.trust_grant` capability declaration: managed hook
    /// key(s) core passes to the plugin's `grantIntegrationTrust` per install.
    #[serde(default, rename = "trust_grant", alias = "trustGrant")]
    pub trust_grant: Option<TrustGrantConfig>,
}

/// Manifest mirror of `crate::provider::IntegrationTrustGrantCapability`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TrustGrantConfig {
    pub managed_keys: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationKind {
    /// Claude-style: edit a JSON file's `hooks` object with curl-back
    /// commands.
    ClaudeSettingsJson,
    /// Provider JS owns a reviewable global-config merge and backup/restore flow.
    ProviderManaged,
    /// No installable integration. PTY parser does it all.
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuleConfig {
    /// Where the codeterm-rule markdown file lives.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SkillsConfig {
    /// Where plugin-contributed skill files get written.
    pub dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UsageConfig {
    /// Slash-command that probes usage in the live TUI (e.g. `/usage`).
    pub probe_command: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CommandsConfig {
    #[serde(default)]
    pub compact: Option<String>,
    #[serde(default)]
    pub fresh_context: Option<String>,
    #[serde(default)]
    pub exit: Option<String>,
    /// Raw key bytes the host should send to interrupt the agent's
    /// current operation (NOT a clean exit — think Esc / Ctrl+C while
    /// the model is generating). Examples: `""` for Esc,
    /// `""` for Ctrl+C.
    #[serde(default)]
    pub stop_key: Option<String>,
    /// Format string with `{model}` placeholder.
    #[serde(default)]
    pub model_switch_format: Option<String>,
    #[serde(default)]
    pub auto_approve_flags: Vec<String>,
    /// Each element may contain `{model}` placeholder.
    #[serde(default)]
    pub model_launch_args_format: Vec<String>,
    /// Each element may contain `{effort}` placeholder. Absent ⇒ provider has no
    /// scriptable reasoning-effort launch flag even if it lists effort levels.
    #[serde(default)]
    pub reasoning_effort_launch_args_format: Vec<String>,
    /// `launchArgsFormat`: full launch command template, used only when the
    /// plugin exports no `buildLaunchCommand`. See LAUNCH_TEMPLATE_PLACEHOLDERS.
    #[serde(default)]
    pub launch_args_format: Option<String>,
    /// `resumeArgsFormat`: resume counterpart for a missing
    /// `buildResumeCommand`. See RESUME_TEMPLATE_PLACEHOLDERS.
    #[serde(default)]
    pub resume_args_format: Option<String>,
}

/// Closed placeholder set for `launchArgsFormat`. `{skipPermissions}` expands
/// to the manifest's `autoApproveFlags`; model/effort flags arrive via `{args}`.
pub const LAUNCH_TEMPLATE_PLACEHOLDERS: &[&str] = &["task", "args", "skipPermissions", "sessionId"];

/// Closed placeholder set for `resumeArgsFormat`.
pub const RESUME_TEMPLATE_PLACEHOLDERS: &[&str] = &["sessionId", "skipPermissions"];

/// First `{name}` in `template` outside `allowed`, or an unterminated `{`.
/// `None` means every placeholder resolves.
pub fn unknown_template_placeholder(template: &str, allowed: &[&str]) -> Option<String> {
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let tail = &rest[open + 1..];
        let Some(close) = tail.find('}') else {
            return Some(rest[open..].to_string());
        };
        let name = &tail[..close];
        if !allowed.contains(&name) {
            return Some(name.to_string());
        }
        rest = &tail[close + 1..];
    }
    None
}

/// Single-pass `{name}` → value substitution — no conditionals, no quoting,
/// and substituted values are never re-scanned for further placeholders.
pub fn substitute_template(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        let Some(close) = tail.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let name = &tail[..close];
        match values.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => out.push_str(value),
            None => out.push_str(&rest[open..open + close + 2]),
        }
        rest = &tail[close + 1..];
    }
    out.push_str(rest);
    out
}

/// CamelCase-friendly mirror of `ModelInfo` for plugin.json authors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SerdeModelInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// See `ModelInfo::available`. Lets a manifest seed ship an alias as
    /// disabled (`false`) until live discovery confirms it for the account.
    #[serde(default)]
    pub available: Option<bool>,
    /// See `ModelInfo::availability`. A probe that could not run reports
    /// `unknown` here rather than collapsing into `available: false`.
    #[serde(default)]
    pub availability: Option<ModelAvailability>,
    /// See `ModelInfo::featured`. Marks a model to lead the picker list.
    #[serde(default)]
    pub featured: Option<bool>,
    /// See `ModelInfo::badge`. Short chip rendered beside the picker label.
    #[serde(default)]
    pub badge: Option<String>,
}

impl From<&SerdeModelInfo> for ModelInfo {
    fn from(s: &SerdeModelInfo) -> Self {
        Self {
            id: s.id.clone(),
            display_name: s.display_name.clone(),
            description: s.description.clone(),
            available: s.available,
            availability: s.availability,
            featured: s.featured,
            badge: s.badge.clone(),
        }
    }
}

/// CamelCase-friendly mirror of `ReasoningEffortInfo` for plugin.json authors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SerdeReasoningEffortInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
}

impl From<&SerdeReasoningEffortInfo> for ReasoningEffortInfo {
    fn from(s: &SerdeReasoningEffortInfo) -> Self {
        Self {
            id: s.id.clone(),
            display_name: s.display_name.clone(),
            description: s.description.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PermissionsConfig {
    #[serde(default)]
    pub transport_events: Vec<String>,
    #[serde(default)]
    pub send_text: bool,
    /// `host.fetch` host allowlist.
    #[serde(default)]
    pub network: NetworkPermission,
    /// `host.exec` binary allowlist.
    #[serde(default)]
    pub subprocess: SubprocessPermission,
    /// Whether the plugin may use its per-plugin secret bucket (`host.secret*`).
    #[serde(default)]
    pub secrets: bool,
    /// Whether a spawned agent may route `codeterm plugin <id> <verb>` to this plugin's `onAgentCommand` (the inverse seam).
    #[serde(default, rename = "agent_commands", alias = "agentCommands")]
    pub agent_commands: bool,
    /// Whether the plugin may create/manage workspaces via `host.workspace*`.
    #[serde(default, rename = "manage_workspaces", alias = "manageWorkspaces")]
    pub manage_workspaces: bool,
    /// Whether the plugin may spawn/drive our agents via `host.agent*`.
    #[serde(default, rename = "spawn_agents", alias = "spawnAgents")]
    pub spawn_agents: bool,
    /// Whether the plugin may read machine-wide KPI aggregates via `host.analytics`.
    #[serde(default)]
    pub analytics: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_model_info_carries_featured_and_badge_into_model_info() {
        let parsed: SerdeModelInfo = serde_json::from_value(serde_json::json!({
            "id": "deepseek-v4-flash",
            "displayName": "DeepSeek V4 Flash",
            "featured": true,
            "badge": "own",
        }))
        .expect("featured/badge parse");

        let model: ModelInfo = (&parsed).into();
        assert_eq!(model.featured, Some(true));
        assert_eq!(model.badge.as_deref(), Some("own"));
        assert!(model.is_featured());
    }

    #[test]
    fn serde_model_info_defaults_featured_and_badge_to_none() {
        let parsed: SerdeModelInfo =
            serde_json::from_value(serde_json::json!({ "id": "m", "displayName": "M" }))
                .expect("minimal model parse");

        let model: ModelInfo = (&parsed).into();
        assert_eq!(model.featured, None);
        assert_eq!(model.badge, None);
        assert!(!model.is_featured());
    }
}
