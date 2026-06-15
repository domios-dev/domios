use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How to wait for agent readiness after PTY spawn.
#[derive(Debug, Clone)]
pub enum ReadinessStrategy {
    PollShellPrompt { timeout: Duration },
    FixedDelay { duration: Duration },
}

/// How to confirm that text submitted to a launched provider was actually
/// processed (Enter consumed, REPL accepted it). Used to gate `OK` on a
/// real receiver-side observation rather than an optimistic write.
#[derive(Debug, Clone)]
pub enum VerifySubmitStrategy {
    /// Wait for a new shell prompt to appear (default for plain shells and
    /// providers that pipe through the shell unchanged).
    ShellPrompt { timeout: Duration },
    /// Wait for the TUI's input box to drain — the typed text disappears
    /// from the bottom rows. Cheap heuristic: poll the last few rows; once
    /// the needle is gone, wait `idle_after_drain` of no-screen-change
    /// before declaring submitted.
    TuiInputDrained {
        timeout: Duration,
        idle_after_drain: Duration,
    },
    /// No verification — for providers where post-submit state is not
    /// observable from the PTY (session-id-injection, headless flag delivery, etc.).
    None,
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
    // Pre-generated session id to use when starting a NEW conversation. Honored only by providers reporting `SessionIdInjection != None` — others
    // ignore it (they generate ids themselves and CodeTerm has to detect after launch).
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
    /// the variant whose `id` matches the returned value. `query_args`
    /// is an optional name → value map forwarded as the invoke arguments
    /// (snake_case keys are remapped to camelCase by Tauri automatically).
    Badge {
        query_command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query_args: Option<serde_json::Value>,
        variants: Vec<BadgeVariant>,
    },

    // ── Actions ──────────────────────────────────────────────────
    /// Button that invokes a backend command. Optional `state_query`
    /// turns it into a multi-state button (Install / Reinstall / Update).
    /// Both `command` and `state_query` use the same `args` payload —
    /// pass `provider_id` here for generic dispatchers that target a
    /// specific provider.
    Action {
        id: String,
        label: String,
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_query: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
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

// Reusable settings schema builders — shared across providers.

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

/// Build the "Usage Tracking" settings section. Auto-injected into the
/// schema for any provider that declares `usage_parser()` — providers
/// don't need to include it manually.
pub fn usage_tracking_section(probe_command: &str) -> SettingField {
    SettingField::Section {
        title: "Usage Tracking (experimental)".into(),
        description: Some(format!(
            "Periodically probe {} in a hidden pane to capture session + weekly limits, \
             then surface them as a pill in the corner of every pane plus a projections \
             popover. Off by default — turn on to enable for this provider. Experimental: \
             the hidden probe spawns a real agent process every 10–50 min and can in rare \
             cases interact awkwardly with active sessions on the same account.",
            probe_command
        )),
        fields: vec![SettingField::Toggle {
            key: "usage_tracking_enabled".into(),
            label: "Enable usage tracking".into(),
            default: false,
            description: None,
        }],
    }
}

/// Build the "Model" settings section from a provider's available models.
pub fn model_select_section(
    description: &str,
    models: Vec<ModelInfo>,
    default: Option<&str>,
) -> SettingField {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_svg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,
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
    /// CLI binary name the provider expects on PATH (e.g. `claude`, `codex`,
    /// `opencode`). When present, the frontend renders an "Installation"
    /// section that polls `which <binary>` and offers an Install button.
    /// Resolved per-platform on the backend at describe-time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_binary: Option<String>,
    /// Shell command for installing the CLI on the current platform. Frontend
    /// pipes it into a fresh terminal pane when the user clicks Install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_command: Option<String>,
    /// Provider-declared in-agent model switch command template. `{model}` is
    /// replaced by the frontend with the selected `ModelInfo.id`. Absent means
    /// the provider may publish models for launch/config surfaces but does not
    /// support an arg-based live session switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_switch_format: Option<String>,
    /// `true` when this provider declares a stop_keystroke — UI shows
    /// a Stop button on the pane that POSTs to `/api/panes/:id/stop`.
    #[serde(default)]
    pub supports_stop: bool,
    /// `true` when the provider can render a pane as chat — either it
    /// stores sessions as tailable JSONL or serves structured chat from
    /// its own store. The UI gates the chat/terminal toggle on this
    /// instead of a hardcoded provider list.
    #[serde(default)]
    pub supports_chat_ui: bool,
    /// Trust status of the bundle backing this provider. `system` =
    /// shipped by CodeTerm, hash matched the build-time table, or was
    /// installed from the official codeterm-core system channel.
    /// `unverified` = same id as a system plugin but the bundle on
    /// disk has a different hash (user-modified or tampered).
    /// `third_party` = plugin id not in the system table.
    /// `unknown` = native (Rust-side) provider, no hash to check.
    #[serde(default)]
    pub trust: ProviderTrust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTrust {
    System,
    Unverified,
    ThirdParty,
    #[default]
    Unknown,
}

/// Current `ProviderDescriptor` schema version. Bump on breaking change.
pub const PROVIDER_DESCRIPTOR_SCHEMA_VERSION: u32 = 5;

/// Trait for agent backend providers (Claude, Codex, Aider, etc.)
pub trait AgentProvider: Send + Sync {
    /// Unique identifier for this provider (e.g. "claude").
    fn id(&self) -> &str;

    /// Human-readable name (e.g. "Claude Code").
    fn display_name(&self) -> &str;

    /// Executable name a user would type to launch this provider from a shell
    /// (e.g. `claude`, `codex`, `opencode`). Used by the input parser to map
    /// typed commands → agent_type. Default falls back to `id()` so trivial
    /// providers don't have to override.
    fn binary_name(&self) -> &str {
        self.id()
    }

    /// Build the shell command to launch this agent.
    fn build_launch_command(&self, params: &AgentLaunchParams) -> String;

    /// How to wait for the agent to be ready after PTY spawn.
    fn readiness_strategy(&self) -> ReadinessStrategy;

    /// How to deliver the task to the agent.
    fn task_delivery(&self) -> TaskDelivery;

    /// Extra delay between launch verify and PtyInput task delivery. Some TUIs
    /// keep emitting output after the launch command completes (status bar,
    /// MCP fetch, splash animations) and only render their input box a few
    /// seconds later — typing the task immediately drops it into the void.
    /// Default is zero; TUI providers override.
    fn task_input_warmup(&self) -> Duration {
        Duration::ZERO
    }

    /// How to verify that submitted text was actually processed. Default is
    /// shell-prompt polling — providers running TUIs or non-observable flows
    /// override (TuiInputDrained / None).
    fn verify_submit_strategy(&self) -> VerifySubmitStrategy {
        VerifySubmitStrategy::ShellPrompt {
            timeout: Duration::from_secs(5),
        }
    }

    /// Detect this agent from the pane/tab title string.
    fn detect_from_title(&self, title: &str) -> bool;

    /// Detect this agent from PTY output text.
    fn detect_from_output(&self, text: &str) -> bool;

    /// Returns true when this provider's TUI is visible in the given screen
    /// text. Used by the restart logic in addition to the universal glyph
    /// check so each provider owns its own text fingerprints. Default is
    /// false — override for providers whose TUI emits recognisable text that
    /// is not covered by box-drawing or block glyphs.
    fn screen_has_tui(&self, _screen_text: &str) -> bool {
        false
    }

    /// Build the path to the session file for this agent.
    fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf>;

    /// Detect the most recent session ID for the given working directory.
    fn detect_session_id(
        &self,
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
    ) -> Option<String>;

    /// Detect a session that belongs to this launch attempt.
    ///
    /// `max_age` is interpreted as "younger than now - max_age". Callers pass
    /// launch age so stale sessions in the same cwd are ignored.
    /// `injected_session_id` is an optional pre-generated id used by providers
    /// that support session-id injection.
    fn detect_launched_session_id(
        &self,
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
        injected_session_id: Option<&str>,
    ) -> Option<String>;

    /// Detect that a provider session for this launch is actually active.
    ///
    /// `screen_text` can be passed when available to avoid false-positive
    /// attachment to stale filesystem sessions. For providers with explicit or
    /// seeded session IDs this is usually `Some(session_id) == session_id`.
    /// Implementations should return the same sid contract as
    /// `detect_launched_session_id`.
    fn detect_started_session(
        &self,
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
        injected_session_id: Option<&str>,
        screen_text: Option<&str>,
    ) -> Option<String>;

    /// Common startup detector used by providers that should only claim a
    /// filesystem-detected session after seeing provider output markers.
    fn detect_started_session_with_markers(
        &self,
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
        injected_session_id: Option<&str>,
        screen_text: Option<&str>,
    ) -> Option<String> {
        let Some(sid) = self.detect_launched_session_id(cwd, exclude, max_age, injected_session_id)
        else {
            return None;
        };

        if injected_session_id.is_some() {
            return Some(sid);
        }

        match screen_text {
            Some(text) if text.trim().is_empty() => Some(sid),
            Some(text) if self.detect_from_output(text) => Some(sid),
            Some(_) => None,
            None => Some(sid),
        }
    }

    /// Whether a session id still has a backing file (JSONL / session store / whatever
    /// the provider keeps). Default implementation is `session_file_path(...).exists()`.
    /// Providers without file-based session storage override this.
    fn session_exists(&self, cwd: &str, session_id: &str) -> bool {
        self.session_file_path(cwd, session_id)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    /// Session creation time, for providers whose sessions have no per-session
    /// file (sqlite-backed, e.g. opencode) so the launch-window birth-time guard
    /// in `detect_launched_session_id` can still reject pre-launch sessions.
    /// Default `None` — file-based providers rely on filesystem timestamps.
    fn session_created_at(&self, _cwd: &str, _session_id: &str) -> Option<std::time::SystemTime> {
        None
    }

    /// Build a command to resume an existing session.
    fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String>;

    /// Cumulative token usage from the provider's session log.
    fn read_usage(&self, _cwd: &str, _session_id: &str) -> Option<UsageSnapshot> {
        None
    }
    fn usage_parser(&self) -> Option<&dyn crate::agent::usage_parser::UsageParser> {
        None
    }

    fn usage_probe_command(&self) -> Option<&str> {
        None
    }

    /// What kind of session-id control this provider supports.
    /// Default is `None` — provider generates its own ids and CodeTerm
    /// has to detect them post-launch via `detect_session_id`.
    fn session_id_injection(&self) -> SessionIdInjection {
        SessionIdInjection::None
    }

    /// Enumerate all past sessions this provider keeps on disk.
    /// Used by the cross-provider session search. Default returns empty —
    /// providers without on-disk session history (or not yet implemented)
    /// contribute nothing and fall out of search silently.
    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        Vec::new()
    }

    /// Extract conversation messages from a session file produced by this
    /// provider. Called by the search pipeline for each candidate session;
    /// returning `vec![]` means this provider can't participate in content
    /// search yet. Must be pure + I/O-only (no locks, no process calls).
    fn parse_session_messages(&self, file_path: &std::path::Path) -> Vec<SessionMessage> {
        let _ = file_path;
        Vec::new()
    }

    /// Resolve the actual session JSONL file for `(cwd, session_id)`. The
    /// chat tailer reads this file with byte-offset bookmarks. Returns
    /// `None` when the provider doesn't store sessions as a single JSONL
    /// (opencode is sqlite-backed — returns `None`; the UI will hide
    /// the toggle for those providers).
    fn session_jsonl_path(&self, _cwd: &str, _session_id: &str) -> Option<std::path::PathBuf> {
        None
    }

    /// Parse new messages from `path` starting at `from_offset` bytes,
    /// returning the delta + new offset. Implementations must be
    /// tolerant of partial last lines (a poll can land mid-write): if
    /// trailing bytes don't terminate in `\n`, hold them back and return
    /// `new_offset` pointing at the start of the incomplete line.
    fn parse_session_delta(
        &self,
        _path: &std::path::Path,
        _from_offset: u64,
    ) -> crate::agent::chat_types::SessionDelta {
        crate::agent::chat_types::SessionDelta {
            new_offset: _from_offset,
            messages: vec![],
        }
    }

    /// Parse provider-specific interactive prompts (AskUserQuestion, ...) out of a
    /// single parsed message. Default: none. Core never branches on provider id —
    /// it asks every provider via this capability.
    fn parse_interactive_events(
        &self,
        _msg: &crate::agent::chat_types::RichMessage,
    ) -> Vec<crate::agent::chat_types::InteractiveElement> {
        Vec::new()
    }

    /// Provider serves chat as structured messages it parses itself (e.g.
    /// sqlite) rather than a tailable JSONL file. When true, the host uses
    /// `read_structured_chat` and a DB poller instead of `session_jsonl_path`
    /// + the byte-offset tailer.
    fn uses_structured_chat(&self) -> bool {
        false
    }

    /// Read structured chat for `(cwd, session_id)`. `cursor` is an opaque
    /// bookmark from a prior call (`None` = from the start). Returns
    /// new-or-updated messages plus the next cursor. Like the JSONL tail,
    /// returning the still-growing tail message again is fine — the UI merges
    /// by message id.
    fn read_structured_chat(
        &self,
        _cwd: &str,
        _session_id: &str,
        _cursor: Option<&str>,
    ) -> crate::agent::chat_types::StructuredChat {
        crate::agent::chat_types::StructuredChat::default()
    }

    /// Whether this agent supports hook-based detection (vs PTY parsing fallback).
    /// Agents with hooks get detected via API endpoints, not PTY output parsing.
    fn has_hooks(&self) -> bool {
        false
    }

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

    /// Extract the agent's human-readable session display name from the
    /// current screen — e.g. Claude Code prints "vivid-bhabha-1234" in its
    /// TUI banner. Captured once per agent run and stored on the pane so
    /// `codeterm pane find <pattern>` can locate panes by friendly name.
    /// Default returns None; providers that emit a stable handle override.
    fn extract_session_name(&self, _screen: &str) -> Option<String> {
        None
    }

    /// Current model family on the live session — short label only.
    fn detect_session_model(&self, _cwd: &str, _session_id: &str) -> Option<String> {
        None
    }

    /// When the TUI silently rolls into a new on-disk session (opencode does
    /// this on `/model` swap), the pane's cached `session_id` goes stale and
    /// `detect_session_model` keeps returning the previous model. Providers
    /// that support this override to point the pane at the live session —
    /// return `Some(new_sid)` when one exists, else `None`.
    fn detect_active_session(&self, _cwd: &str, _current_sid: &str) -> Option<String> {
        None
    }

    /// Filter TUI noise from agent output. Returns true if the text is noise (should be dropped).
    fn is_output_noise(&self, text: &str) -> bool {
        let _ = text;
        false
    }

    /// Bottom rows occupied by status bar/chrome, excluded from idle detection hash.
    fn status_bar_rows(&self) -> u16 {
        0
    }

    /// Quick pre-filter on raw PTY chunks (must be fast).
    /// If true, full detection runs on the VT100 screen content.
    fn has_event_markers(&self, chunk: &str) -> bool {
        chunk.contains("❯")
            || chunk.contains("proceed")
            || chunk.contains("Esc to cancel")
            || chunk.contains("Tab to amend")
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

    // In-agent control commands — defaults return None/empty.

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
        self.model_switch_format()
            .map(|fmt| fmt.replace("{model}", _model))
    }

    /// In-agent model switch command template. Frontend consumers use the
    /// template to render provider-agnostic model pickers without hardcoding
    /// provider slash commands.
    fn model_switch_format(&self) -> Option<&str> {
        None
    }

    /// In-agent command that exits the REPL cleanly, if supported.
    fn exit_command(&self) -> Option<&str> {
        None
    }

    /// Raw key bytes the host writes into the PTY to interrupt the
    /// agent's current generation without exiting. Most TUIs accept
    /// Esc (`""`); cursor-agent listens for Ctrl+C (`""`).
    /// Returning `None` means the provider has no in-pane stop hook
    /// and the UI should hide the Stop affordance for that pane.
    fn stop_keystroke(&self) -> Option<&str> {
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

    // CodeTerm integration varies: Claude uses hooks for permission/task callbacks;
    // Codex/OpenCode are PTY-observed and need no extra provider config.

    /// Status of CodeTerm's notification integration with this provider —
    /// returned as opaque JSON so each provider shapes it for its own UI.
    /// Default: providers without external integration report `status: "ok"`
    /// and the Badge renders "available".
    fn check_integration(&self) -> serde_json::Value {
        serde_json::json!({ "status": "ok" })
    }

    /// Install CodeTerm's notification integration. `api_port` is the local
    /// CodeTerm HTTP port that callbacks should target. Default no-op.
    fn install_integration(&self, _api_port: u16) -> Result<(), String> {
        Ok(())
    }

    fn uninstall_integration(&self) -> Result<(), String> {
        Ok(())
    }

    /// Idempotent auto-install on startup. Called once per registered
    /// provider during boot. Default no-op.
    fn ensure_integration_on_startup(&self, _api_port: u16) {}

    // Project rules: only Claude reads `~/.claude/rules/codeterm.md`.
    // Others inject rules elsewhere, so installed defaults to true when file is absent.

    fn check_rule_status(&self) -> serde_json::Value {
        serde_json::json!({ "installed": true, "update_available": false })
    }

    fn install_rule(&self) -> Result<(), String> {
        Ok(())
    }

    // Self-presentation: frontend reads provider tabs, models, and settings purely from these + `describe()`. Defaults stay empty.

    /// Models this provider offers (e.g. sonnet, opus, haiku for Claude).
    fn available_models(&self) -> Vec<ModelInfo> {
        Vec::new()
    }

    /// Default model id, must match one of `available_models()` if non-None.
    /// Returns `String` (not `&str`) so providers can resolve through the
    /// user-editable `model_catalog` without lifetime headaches.
    fn default_model(&self) -> Option<String> {
        None
    }

    fn icon_hint(&self) -> Option<&str> {
        None
    }

    fn icon_svg(&self) -> Option<&str> {
        None
    }

    fn icon_color(&self) -> Option<&str> {
        None
    }

    /// Name of the binary the provider's launch commands invoke (e.g. `claude`,
    /// `codex`, `opencode`). When `Some`, CodeTerm checks PATH availability so
    /// the Settings UI can render an "install" affordance for missing CLIs.
    /// Default `None` — provider opts out of installation tracking.
    fn cli_binary(&self) -> Option<&str> {
        None
    }

    /// Per-platform install command run inside a fresh terminal pane when the
    /// user clicks "Install" in Settings. `platform` is one of `"macos"`,
    /// `"linux"`, `"windows"`. Return `None` for platforms where you don't ship
    /// an installer recipe.
    fn install_command(&self, _platform: &str) -> Option<String> {
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
        let platform = current_platform_tag();
        let mut settings = self.settings_schema();
        if self.usage_parser().is_some() {
            let probe_cmd = self.usage_probe_command().unwrap_or("/usage");
            settings.push(usage_tracking_section(probe_cmd));
        }
        ProviderDescriptor {
            schema_version: PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
            id: self.id().to_string(),
            display_name: self.display_name().to_string(),
            icon_hint: self.icon_hint().map(|s| s.to_string()),
            icon_svg: self.icon_svg().map(|s| s.to_string()),
            icon_color: self.icon_color().map(|s| s.to_string()),
            available_models: self.available_models(),
            default_model: self.default_model(),
            settings_schema: settings,
            notifications_schema: self.notifications_schema(),
            supports_lazy_skills: self.supports_lazy_skills(),
            has_hooks: self.has_hooks(),
            preserves_context_on_resume: self.preserves_context_on_resume(),
            cli_binary: self.cli_binary().map(|s| s.to_string()),
            install_command: self.install_command(platform),
            model_switch_format: self.model_switch_format().map(|s| s.to_string()),
            supports_stop: self.stop_keystroke().is_some(),
            supports_chat_ui: self.supports_chat_ui(),
            trust: self.trust(),
        }
    }

    /// Whether a pane running this provider can be shown as chat. Defaults
    /// to "yes if it serves structured chat"; JSONL providers override to
    /// `true`. The frontend reads this off the descriptor — no hardcoded
    /// provider list.
    fn supports_chat_ui(&self) -> bool {
        self.uses_structured_chat()
    }

    /// Bundle trust status. Default impl returns `Unknown` (native
    /// providers — no hash to check). JS-backed providers override
    /// to return System/Unverified/ThirdParty based on the
    /// build-time hash table.
    fn trust(&self) -> ProviderTrust {
        ProviderTrust::Unknown
    }

    /// Whether this is a native (Rust) implementation. JS drop-ins override to
    /// `false`. The registry relies on this so it never records a JS provider as
    /// the "native fallback" another provider delegates to: a JS-only provider
    /// re-registered on hot-reload would otherwise become its own fallback, and
    /// `JsProvider::native()` returning itself makes capability delegation
    /// recurse until the stack overflows.
    fn is_native(&self) -> bool {
        true
    }
}

/// Cumulative token usage for one agent session.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct UsageSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub message_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Normalised platform tag used by `install_command(platform)`.
pub fn current_platform_tag() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        other => other,
    }
}

#[cfg(test)]
mod tests;
