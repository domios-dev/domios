use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How to wait for agent readiness after PTY spawn.
#[derive(Debug, Clone)]
pub enum ReadinessStrategy {
    PollShellPrompt { timeout: Duration },
    FixedDelay { duration: Duration },
}

/// How to deliver the task to the agent.
#[derive(Debug, Clone)]
pub enum TaskDelivery {
    /// Task is embedded in the launch command (Claude's temp file approach).
    InlineFlag,
    /// Task is sent as PTY text after agent is ready.
    PtyInput,
}

#[derive(Debug, Clone, Default)]
pub struct AgentLaunchParams {
    pub agent_name: Option<String>,
    pub task: Option<String>,
    pub args: Option<Vec<String>>,
    pub skip_permissions: bool,
    // True for one-shot hidden panes; providers pick non-interactive delivery.
    pub headless: bool,
    // Pre-generated session id to use when starting a NEW conversation.
    // Honored only by providers reporting `SessionIdInjection != None` —
    // others ignore it (they generate ids themselves and CodeTerm has to
    // detect after launch).
    pub session_id: Option<String>,
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

// ── Plugin self-presentation types (Chunk B.1) ─────────────────────
//
// `ProviderDescriptor` is the wire format every plugin reports through
// `AgentProvider::describe()`. The frontend reads ONE list and uses it
// to render Settings panels, Sidebar menus, command palette entries —
// no hardcoded provider knowledge anywhere on the JS side.

/// Single model offered by an agent provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

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

/// Declarative settings UI primitives. The frontend ships ONE
/// `<SchemaRenderer />` component that knows how to render each variant.
/// Plugin authors describe their settings panel as a `Vec<SettingField>`
/// — no JS, no per-plugin React component, no hardcoded fallback.
///
/// **Schema completeness rule:** if a real plugin needs a UI pattern not
/// covered here, ADD A NEW VARIANT — never carve out an escape hatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SettingField {
    // ── Grouping ─────────────────────────────────────────────────
    /// Card with title + description + nested fields.
    Section {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        fields: Vec<SettingField>,
    },

    // ── Inputs ───────────────────────────────────────────────────
    Toggle {
        /// Config path relative to the plugin's namespace
        /// (e.g. `"resume_sessions"` → `agents.<id>.resume_sessions`).
        key: String,
        label: String,
        default: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Text {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Number {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Select {
        key: String,
        label: String,
        /// `(value, display)` pairs.
        options: Vec<(String, String)>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// Read-only field that reports an env var the user must set externally.
    ApiKey {
        env_var: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        link: Option<String>,
    },

    // ── Display ──────────────────────────────────────────────────
    /// Static documentation block. Body supports inline `code` and links.
    Info {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        body: String,
        #[serde(default = "NoteLevel::default_info")]
        level: NoteLevel,
    },
    /// Filesystem path with an "Open folder/file" button.
    PathLink {
        label: String,
        path: String,
        open: PathOpenKind,
    },
    /// Polled status badge: frontend invokes `query_command` and renders
    /// the variant whose `id` matches the returned value.
    Badge {
        query_command: String,
        variants: Vec<BadgeVariant>,
    },

    // ── Actions ──────────────────────────────────────────────────
    /// Button that invokes a backend command. Optional `state_query`
    /// turns it into a multi-state button (Install / Reinstall / Update).
    Action {
        id: String,
        label: String,
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_query: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confirm: Option<String>,
        #[serde(default = "ActionStyle::default_primary")]
        style: ActionStyle,
    },

    // ── Conditional visibility ───────────────────────────────────
    /// Render `then` only when the named field equals `equals`.
    When {
        field: String,
        equals: serde_json::Value,
        then: Vec<SettingField>,
    },
}

impl NoteLevel {
    fn default_info() -> Self {
        NoteLevel::Info
    }
}

impl ActionStyle {
    fn default_primary() -> Self {
        ActionStyle::Primary
    }
}

// ── Reusable settings schema builders ───────────────────────────────
//
// Shared across providers to avoid duplicating identical UI sections.

/// Build the "Session Resume" settings section common to all providers.
/// `skip_flag` is the provider-specific CLI flag for skipping approvals.
pub fn session_resume_section(description: &str, skip_flag: &str) -> SettingField {
    SettingField::Section {
        title: "Session Resume".into(),
        description: Some(description.into()),
        fields: vec![
            SettingField::Toggle {
                key: "resume_sessions".into(),
                label: "Resume previous sessions".into(),
                default: true,
                description: None,
            },
            SettingField::Toggle {
                key: "resume_skip_permissions".into(),
                label: format!("Resume with {}", skip_flag),
                default: true,
                description: None,
            },
            SettingField::When {
                field: "resume_skip_permissions".into(),
                equals: serde_json::Value::Bool(true),
                then: vec![SettingField::Info {
                    title: None,
                    body: "⚠ Agent will run without asking for tool approvals. Use with caution."
                        .into(),
                    level: NoteLevel::Warn,
                }],
            },
        ],
    }
}

/// Build the "Model" settings section from a provider's available models.
pub fn model_select_section(description: &str, models: Vec<ModelInfo>, default: Option<&str>) -> SettingField {
    SettingField::Section {
        title: "Model".into(),
        description: Some(description.into()),
        fields: vec![SettingField::Select {
            key: "default_model".into(),
            label: "Default model".into(),
            options: models.into_iter().map(|m| (m.id, m.display_name)).collect(),
            default: default.map(|s| s.to_string()),
            description: None,
        }],
    }
}

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
    pub available_models: Vec<ModelInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
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
    pub preserves_context_on_resume: bool,
}

/// Current `ProviderDescriptor` schema version. Bump on breaking change.
pub const PROVIDER_DESCRIPTOR_SCHEMA_VERSION: u32 = 1;

/// Trait for agent backend providers (Claude, Codex, Aider, etc.)
pub trait AgentProvider: Send + Sync {
    /// Unique identifier for this provider (e.g. "claude").
    fn id(&self) -> &str;

    /// Human-readable name (e.g. "Claude Code").
    fn display_name(&self) -> &str;

    /// Build the shell command to launch this agent.
    fn build_launch_command(&self, params: &AgentLaunchParams) -> String;

    /// How to wait for the agent to be ready after PTY spawn.
    fn readiness_strategy(&self) -> ReadinessStrategy;

    /// How to deliver the task to the agent.
    fn task_delivery(&self) -> TaskDelivery;

    /// Detect this agent from the pane/tab title string.
    fn detect_from_title(&self, title: &str) -> bool;

    /// Detect this agent from PTY output text.
    fn detect_from_output(&self, text: &str) -> bool;

    /// Build the path to the session file for this agent.
    fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf>;

    /// Detect the most recent session ID for the given working directory.
    fn detect_session_id(&self, cwd: &str, exclude: &[String], max_age: Option<Duration>) -> Option<String>;

    /// Whether a session id still has a backing file (JSONL / session store / whatever
    /// the provider keeps). Default implementation is `session_file_path(...).exists()`.
    /// Providers without file-based session storage override this.
    fn session_exists(&self, cwd: &str, session_id: &str) -> bool {
        self.session_file_path(cwd, session_id)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    /// Build a command to resume an existing session.
    fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String>;

    /// What kind of session-id control this provider supports.
    /// Default is `None` — provider generates its own ids and CodeTerm
    /// has to detect them post-launch via `detect_session_id`.
    fn session_id_injection(&self) -> SessionIdInjection {
        SessionIdInjection::None
    }

    /// Whether this agent supports hook-based detection (vs PTY parsing fallback).
    /// Agents with hooks get detected via API endpoints, not PTY output parsing.
    fn has_hooks(&self) -> bool { false }

    /// Build raw bytes to navigate to and confirm a prompt option.
    /// `option_index` is 0-based (0 = first option, already selected).
    /// Default: arrow down keys + Enter (standard ANSI).
    /// Providers override for agent-specific navigation (e.g. vim j/k).
    fn build_prompt_response(&self, option_index: usize) -> Vec<Vec<u8>> {
        let mut steps = Vec::new();
        let arrow_down = b"\x1b[B".to_vec(); // ESC [ B
        for _ in 0..option_index {
            steps.push(arrow_down.clone());
        }
        steps.push(b"\r".to_vec()); // Enter
        steps
    }

    /// Filter TUI noise from agent output. Returns true if the text is noise (should be dropped).
    fn is_output_noise(&self, text: &str) -> bool { let _ = text; false }

    /// Bottom rows occupied by status bar/chrome, excluded from idle detection hash.
    fn status_bar_rows(&self) -> u16 { 0 }


    /// Quick pre-filter on raw PTY chunks (must be fast).
    /// If true, full detection runs on the VT100 screen content.
    fn has_event_markers(&self, chunk: &str) -> bool {
        chunk.contains("❯") || chunk.contains("proceed")
            || chunk.contains("Esc to cancel") || chunk.contains("Tab to amend")
            || (chunk.contains("1.") && chunk.contains("2."))
    }

    fn detect_events(&self, text: &str) -> Vec<crate::agent::detector::AgentEvent> {
        let mut events = Vec::new();
        if crate::agent::detector::detect_interactive_prompt(text) {
            events.push(crate::agent::detector::AgentEvent::PermissionRequest);
        }
        events
    }

    fn parse_prompt(&self, text: &str) -> Option<(String, Vec<(String, String)>)> {
        crate::agent::detector::parse_permission_prompt(text)
    }

    // ── Agent-specific CLI commands ──────────────────────────────────
    //
    // These carry per-provider knowledge of in-agent control commands.
    // Default implementations return None / empty so a minimal provider
    // still compiles; real providers override the ones that apply.

    /// Flags to pass to the CLI so the agent auto-approves tool calls.
    fn auto_approve_flags(&self) -> Vec<String> {
        Vec::new()
    }

    /// CLI flags to select a specific model at launch time.
    /// Each provider knows its own flag format (e.g. `--model haiku` for Claude,
    /// `-m gpt-5.4-mini` for Codex). Returns empty vec if model is empty/default.
    fn model_launch_args(&self, model: &str) -> Vec<String> {
        if model.is_empty() {
            return Vec::new();
        }
        vec!["--model".into(), model.into()]
    }

    /// In-agent command that triggers context compaction, if supported.
    fn compact_command(&self) -> Option<&str> {
        None
    }

    /// In-agent command that switches the underlying model, if supported.
    fn model_switch_command(&self, _model: &str) -> Option<String> {
        None
    }

    /// In-agent command that exits the REPL cleanly, if supported.
    fn exit_command(&self) -> Option<&str> {
        None
    }

    // ── Plugin skill delivery ────────────────────────────────────────

    /// Install a plugin-contributed skill file for this agent. Providers
    /// that can't carry skills (system-prompt-only agents) ignore this.
    fn install_skills(&self, _skills: &[crate::plugin::SkillFile]) -> Result<(), String> {
        Ok(())
    }

    /// Remove previously installed skills owned by the given plugin ids.
    fn uninstall_skills(&self, _plugin_ids: &[&str]) -> Result<(), String> {
        Ok(())
    }

    /// Whether this provider can lazy-load skills via trigger words
    /// (Claude Skill tool). Agents without lazy loading fall back to
    /// merging skills into the system prompt up-front.
    fn supports_lazy_skills(&self) -> bool {
        false
    }

    /// Install the mem bridge payload in whatever slot this provider
    /// reads at session start. MUST NOT write into user-owned skill dirs
    /// (`~/.claude/skills/`, `~/.agents/skills/`).
    fn install_bridge_rule(&self, _payload: &str) -> Result<(), String> {
        Ok(())
    }

    fn uninstall_bridge_rule(&self) -> Result<(), String> {
        Ok(())
    }

    // ── Self-presentation metadata (Chunk B.1) ───────────────────────
    //
    // The frontend renders provider tabs, model dropdowns, and settings
    // panels purely from these methods + `describe()`. Default impls are
    // empty so existing providers don't break; real providers override
    // what they actually offer.

    /// Models this provider offers (e.g. sonnet, opus, haiku for Claude).
    fn available_models(&self) -> Vec<ModelInfo> {
        Vec::new()
    }

    /// Default model id, must match one of `available_models()` if non-None.
    fn default_model(&self) -> Option<&str> {
        None
    }

    /// Optional icon hint (emoji or single character) for the provider tab.
    fn icon_hint(&self) -> Option<&str> {
        None
    }

    /// Declarative settings panel for the per-provider Settings → Agents tab.
    /// Frontend renders generically via `<SchemaRenderer />`.
    fn settings_schema(&self) -> Vec<SettingField> {
        Vec::new()
    }

    /// Declarative schema contributed to the global Settings → Notifications
    /// tab. Each provider that wants to plug into notifications (e.g. Claude
    /// installing hook handlers) returns a non-empty schema; the renderer
    /// composes one section per contributing provider. Default empty.
    fn notifications_schema(&self) -> Vec<SettingField> {
        Vec::new()
    }

    /// Whether resuming a session preserves the agent's prior context.
    /// Claude=true (JSONL replay), Codex=false. Replaces the
    /// `provider_name == "claude"` literal in `service/general_agent.rs`.
    fn preserves_context_on_resume(&self) -> bool {
        true
    }

    /// Build the wire descriptor reported to the frontend.
    ///
    /// **Contract:** this method MUST be pure, cheap, and free of I/O.
    /// No locks, no `.await`, no remote calls. It is invoked while
    /// holding a read lock on the agent registry, and the result is
    /// serialized to JSON for REST/Socket/Tauri callers. The default
    /// impl just assembles the other metadata methods — providers
    /// rarely need to override it.
    fn describe(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            schema_version: PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
            id: self.id().to_string(),
            display_name: self.display_name().to_string(),
            icon_hint: self.icon_hint().map(|s| s.to_string()),
            available_models: self.available_models(),
            default_model: self.default_model().map(|s| s.to_string()),
            settings_schema: self.settings_schema(),
            notifications_schema: self.notifications_schema(),
            supports_lazy_skills: self.supports_lazy_skills(),
            has_hooks: self.has_hooks(),
            preserves_context_on_resume: self.preserves_context_on_resume(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip golden test for the wire format.
    /// Catches accidental field renames or breaking serde changes
    /// before they reach external consumers.
    #[test]
    fn provider_descriptor_round_trip() {
        let original = ProviderDescriptor {
            schema_version: PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
            id: "test".into(),
            display_name: "Test Provider".into(),
            icon_hint: Some("T".into()),
            available_models: vec![ModelInfo {
                id: "model-1".into(),
                display_name: "Model One".into(),
                description: Some("first model".into()),
            }],
            default_model: Some("model-1".into()),
            settings_schema: vec![
                SettingField::Section {
                    title: "Group".into(),
                    description: Some("desc".into()),
                    fields: vec![
                        SettingField::Toggle {
                            key: "flag".into(),
                            label: "Flag".into(),
                            default: true,
                            description: None,
                        },
                        SettingField::When {
                            field: "flag".into(),
                            equals: serde_json::json!(true),
                            then: vec![SettingField::Info {
                                title: None,
                                body: "Visible when flag is on".into(),
                                level: NoteLevel::Warn,
                            }],
                        },
                    ],
                },
                SettingField::Action {
                    id: "do_thing".into(),
                    label: "Do Thing".into(),
                    command: "test_command".into(),
                    state_query: Some("query_state".into()),
                    confirm: None,
                    style: ActionStyle::Primary,
                },
                SettingField::Badge {
                    query_command: "check_state".into(),
                    variants: vec![BadgeVariant {
                        id: "ok".into(),
                        label: "OK".into(),
                        style: BadgeStyle::Green,
                    }],
                },
            ],
            notifications_schema: vec![SettingField::Section {
                title: "Notify".into(),
                description: None,
                fields: vec![SettingField::Toggle {
                    key: "notify_on_idle".into(),
                    label: "Notify on idle".into(),
                    default: true,
                    description: None,
                }],
            }],
            supports_lazy_skills: true,
            has_hooks: true,
            preserves_context_on_resume: true,
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: ProviderDescriptor =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, decoded, "round-trip must be lossless");
    }

    #[test]
    fn provider_descriptor_rejects_unknown_fields() {
        // schema_version protects against silent breakage; deny_unknown_fields
        // protects against typos in field names.
        let bad = r#"{
            "schema_version": 1,
            "id": "test",
            "display_name": "Test",
            "available_models": [],
            "settings_schema": [],
            "notifications_schema": [],
            "supports_lazy_skills": false,
            "has_hooks": false,
            "preserves_context_on_resume": true,
            "extra_unknown_field": "boom"
        }"#;
        let result: Result<ProviderDescriptor, _> = serde_json::from_str(bad);
        assert!(result.is_err(), "deny_unknown_fields must reject extras");
    }

    /// Minimal stub provider — exercises `describe()` default impl through
    /// the trait, ensuring it compiles and assembles correctly.
    struct StubProvider;

    impl AgentProvider for StubProvider {
        fn id(&self) -> &str { "stub" }
        fn display_name(&self) -> &str { "Stub" }
        fn build_launch_command(&self, _: &AgentLaunchParams) -> String { String::new() }
        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::FixedDelay { duration: Duration::from_secs(0) }
        }
        fn task_delivery(&self) -> TaskDelivery { TaskDelivery::PtyInput }
        fn detect_from_title(&self, _: &str) -> bool { false }
        fn detect_from_output(&self, _: &str) -> bool { false }
        fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> { None }
        fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> { None }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> { None }
    }

    #[test]
    fn default_describe_assembles_from_methods() {
        let p = StubProvider;
        let desc = p.describe();
        assert_eq!(desc.schema_version, PROVIDER_DESCRIPTOR_SCHEMA_VERSION);
        assert_eq!(desc.id, "stub");
        assert_eq!(desc.display_name, "Stub");
        assert!(desc.available_models.is_empty());
        assert!(desc.settings_schema.is_empty());
        assert!(desc.preserves_context_on_resume);  // default true
        assert!(!desc.supports_lazy_skills);          // default false
        assert!(!desc.has_hooks);                     // default false
    }
}
