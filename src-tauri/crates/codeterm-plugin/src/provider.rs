use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
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
    /// Submit through an interactive TUI. The PTY boundary waits for a
    /// rendered draft before Enter, then observes the provider turn.
    TuiInputDrained,
    /// No verification — for providers where post-submit state is not
    /// observable from the PTY (session-id-injection, headless flag delivery, etc.).
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusySubmitStrategy {
    SubmitNormally,
    Key(String),
}

/// How to deliver the task to the agent.
#[derive(Debug, Clone)]
pub enum TaskDelivery {
    /// Task is embedded in the launch command (Claude's temp file approach).
    InlineFlag,
    /// Task is sent as PTY text after agent is ready.
    PtyInput,
}

/// The single visible turn that starts an interactive provider session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StarterPrompt {
    #[default]
    AssignedTask,
    IdleWakeup,
    TeamBootstrap,
}

impl StarterPrompt {
    pub fn host_text(self) -> Option<&'static str> {
        match self {
            Self::TeamBootstrap => Some(crate::prompts::TEAM_BOOTSTRAP_STARTER.trim()),
            Self::AssignedTask | Self::IdleWakeup => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AgentLaunchParams {
    pub agent_name: Option<String>,
    pub task: Option<String>,
    pub starter_prompt: StarterPrompt,
    pub args: Option<Vec<String>>,
    pub skip_permissions: bool,
    // ignore it (they generate ids themselves and CodeTerm has to detect after launch).
    pub session_id: Option<String>,
    /// Fixed local server the host wants the agent's structured-prompt handle to
    /// listen on (opencode `--hostname/--port`). Set by the host only for
    /// providers that `supports_structured_prompts`; the plugin maps it to the
    /// provider's server flags. Fixes the W3 leading-dash `--args` ergonomics
    /// gap — the host allocates the port instead of the caller hand-passing args.
    pub structured_prompt_server: Option<StructuredPromptServer>,
}

/// A provider-owned response for its directory trust prompt. Core only asks
/// for this after the spawn edge has attested the exact created directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryTrustPromptResponse {
    pub steps: Vec<Vec<u8>>,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Account-can-select-this-now. `None` (common) ⇒ no info, treat as available;
    /// `Some(false)` greys it out in the picker. Set by live discovery; usually `None` in static seeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
}

/// Single reasoning-effort level a provider supports (e.g. codex low/medium/high).
/// Mirrors `ModelInfo`: capability-declared, surfaced in discovery/API/UI. An
/// empty `reasoning_efforts` list means the provider exposes no effort selector.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReasoningEffortInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Structured rejection when a requested reasoning effort is not supported by a
/// provider. Carries the exact supported set so callers surface an honest error
/// instead of silently downgrading. Serializable for API/CLI/report payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ReasoningEffortRejection {
    pub provider: String,
    pub requested: String,
    pub supported: Vec<String>,
}

impl std::fmt::Display for ReasoningEffortRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.supported.is_empty() {
            write!(
                f,
                "provider '{}' does not support reasoning-effort selection (requested '{}')",
                self.provider, self.requested
            )
        } else {
            write!(
                f,
                "provider '{}' does not support reasoning effort '{}'; supported: {}",
                self.provider,
                self.requested,
                self.supported.join(", ")
            )
        }
    }
}

impl std::error::Error for ReasoningEffortRejection {}

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

mod settings;
pub use settings::*;

/// Which runtime drives a provider's tab: a PTY process or a JS chatBackend.
/// Replaces the old injected `kind` string and is the spawn discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTransport {
    Pty,
    ChatBackend,
}

/// What behavior surface a provider exposes for configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum BehaviorConfig {
    #[default]
    None,
    OwnPromptSet,
    PresetsAndSystemPrompt,
    Graph,
}

/// How a system prompt reaches the provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemPromptDelivery {
    CliFlag(String),
    SessionConfig,
    External,
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
    /// CLI binary name the provider expects on PATH (e.g. `claude`, `codex`,
    /// `opencode`). When present, the frontend renders an "Installation"
    /// section that polls `which <binary>` and offers an Install button.
    /// Resolved per-platform on the backend at describe-time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_binary: Option<String>,
    /// Shell command for installing the CLI on the current platform. Frontend
    /// pipes it into a fresh terminal tab when the user clicks Install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_command: Option<String>,
    /// Provider-declared in-agent model switch command template. `{model}` is
    /// replaced by the frontend with the selected `ModelInfo.id`. Absent means
    /// the provider may publish models for launch/config surfaces but does not
    /// support an arg-based live session switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_switch_format: Option<String>,
    /// Provider-declared command that starts a fresh context in the same tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_context_command: Option<String>,
    /// Provider-declared in-agent command that compacts (summarizes) context.
    /// Consumed capability-keyed by the orchestrator refresh cycle — never a
    /// provider-id literal. Absent means no compact capability (mode-B fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_command: Option<String>,
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
    /// `true` when a chatBackend plugin implements watcher mode (manifest
    /// `chatBackend: { watcher: true }`). Always `false` for PTY transports.
    #[serde(default)]
    pub supports_watcher: bool,
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
pub const PROVIDER_DESCRIPTOR_SCHEMA_VERSION: u32 = 10;

/// Trait for agent backend providers (Claude, Codex, Aider, etc.)
#[async_trait]
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

    fn starter_prompt_text(&self, _: StarterPrompt) -> Option<String> {
        None
    }

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

    fn busy_submit_strategy(&self) -> BusySubmitStrategy {
        BusySubmitStrategy::SubmitNormally
    }

    /// Detect this agent from the tab/tab title string.
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

    /// Whether a session id still has a backing file (JSONL / session store / whatever
    /// the provider keeps). Default implementation is `session_file_path(...).exists()`.
    /// Providers without file-based session storage override this.
    fn session_exists(&self, cwd: &str, session_id: &str) -> bool {
        self.session_file_path(cwd, session_id)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    /// Session creation time, for providers whose sessions have no per-session
    /// file (sqlite-backed, e.g. opencode).
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
    fn usage_parser(&self) -> Option<&dyn crate::usage_parser::UsageParser> {
        None
    }

    fn usage_probe_command(&self) -> Option<&str> {
        None
    }

    /// Non-TUI usage fetch: providers whose usage lives behind a CLI/API rather
    /// than a `/usage` TUI screen (e.g. cursor) return the raw payload here. The
    /// poller feeds it to `usage_parser()` exactly like TUI-captured text but
    /// skips spawning a hidden agent tab. `Some(Err)` is an honest fetch
    /// failure; `None` (default) means "use the TUI probe path".
    fn fetch_usage(&self) -> Option<Result<String, String>> {
        None
    }

    /// Cheap predicate (no I/O) for "usage comes from a CLI/API fetch, not a
    /// `/usage` TUI screen". The poller probes these without a visible tab —
    /// there's no terminal to gate on. Default `false` (TUI-probe providers).
    fn usage_is_fetch_based(&self) -> bool {
        false
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
    ) -> codeterm_types::agent::chat_types::SessionDelta {
        codeterm_types::agent::chat_types::SessionDelta {
            new_offset: _from_offset,
            messages: vec![],
        }
    }

    /// Parse provider-specific interactive prompts (AskUserQuestion, ...) out of a
    /// single parsed message. Default: none. Core never branches on provider id —
    /// it asks every provider via this capability.
    fn parse_interactive_events(
        &self,
        _msg: &codeterm_types::agent::chat_types::RichMessage,
    ) -> Vec<codeterm_types::agent::chat_types::InteractiveElement> {
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
    ) -> codeterm_types::agent::chat_types::StructuredChat {
        codeterm_types::agent::chat_types::StructuredChat::default()
    }

    /// Whether this agent supports hook-based detection (vs PTY parsing fallback).
    /// Agents with hooks get detected via API endpoints, not PTY output parsing.
    fn has_hooks(&self) -> bool {
        false
    }

    /// Build raw bytes to navigate to and confirm a prompt option.
    /// `option_index` is 0-based (0 = first option). `screen` is the cleaned
    /// terminal text the prompt is on — providers whose highlight isn't the top
    /// row (cursor's /model lands on the CURRENT model) read it to navigate
    /// relative to the live highlight. Default ignores it: arrow down + Enter.
    /// Providers override for agent-specific navigation (e.g. vim j/k).
    fn build_prompt_response(&self, option_index: usize, _screen: &str) -> Vec<Vec<u8>> {
        let mut steps = Vec::new();
        let arrow_down = b"\x1b[B".to_vec(); // ESC [ B
        for _ in 0..option_index {
            steps.push(arrow_down.clone());
        }
        steps.push(b"\r".to_vec()); // Enter
        steps
    }

    /// Recognize and answer this provider's directory-trust prompt. `None`
    /// means the screen must remain a normal surfaced prompt.
    fn directory_trust_prompt_response(
        &self,
        _prompt: &codeterm_types::pty::PendingPrompt,
        _screen: &str,
    ) -> Option<DirectoryTrustPromptResponse> {
        None
    }

    /// Extract the agent's human-readable session display name from the
    /// current screen — e.g. Claude Code prints "vivid-bhabha-1234" in its
    /// TUI banner. Captured once per agent run and stored on the tab so
    /// `codeterm tab find <pattern>` can locate tabs by friendly name.
    /// Default returns None; providers that emit a stable handle override.
    fn extract_session_name(&self, _screen: &str) -> Option<String> {
        None
    }

    /// Current model family on the live session — short label only.
    fn detect_session_model(&self, _cwd: &str, _session_id: &str) -> Option<String> {
        None
    }

    /// When the TUI silently rolls into a new on-disk session (opencode does
    /// this on `/model` swap), the tab's cached `session_id` goes stale and
    /// `detect_session_model` keeps returning the previous model. Providers
    /// that support this override to point the tab at the live session —
    /// return `Some(new_sid)` when one exists, else `None`.
    fn detect_active_session(&self, _cwd: &str, _current_sid: &str) -> Option<String> {
        None
    }

    /// Classify a parked tab from a generic snapshot (AS1 seam). Optional — default
    /// `None`. An opting-in provider runs its own matchers and returns a generic
    /// verdict; core sees only that, never the fingerprints. Called only when parked.
    fn classify_tab_state(
        &self,
        _snapshot: &crate::tab_state::TabStateSnapshot,
    ) -> Option<crate::tab_state::TabStateClassification> {
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

    /// Whether the shared structural agent-state classifier deterministically
    /// covers both this provider's ready prompt and active-turn UI.
    fn supports_agent_state_status_reconciliation(&self) -> bool {
        false
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

    fn detect_events(&self, text: &str) -> Vec<crate::detector::AgentEvent> {
        let mut events = Vec::new();
        if crate::detector::detect_interactive_prompt(text) {
            events.push(crate::detector::AgentEvent::PermissionRequest);
        }
        events
    }

    fn parse_prompt(&self, text: &str) -> Option<codeterm_types::pty::PendingPrompt> {
        crate::detector::parse_permission_prompt(text)
            .map(|(question, options)| crate::prompt::build_prompt(question, options))
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

    /// Reasoning-effort levels this provider supports. Empty (default) means the
    /// provider has no effort capability — callers must not offer a selector.
    fn reasoning_efforts(&self) -> Vec<ReasoningEffortInfo> {
        Vec::new()
    }

    /// Default reasoning-effort id, must match one of `reasoning_efforts()`.
    fn default_reasoning_effort(&self) -> Option<String> {
        None
    }

    /// CLI flags to select a reasoning effort at launch. Each provider knows its
    /// own flag format (e.g. codex `-c model_reasoning_effort="high"`). Returns
    /// empty when the effort is empty; capability-gating is the caller's job via
    /// `validate_reasoning_effort`. Default: no capability ⇒ no flags.
    fn reasoning_effort_launch_args(&self, _effort: &str) -> Vec<String> {
        Vec::new()
    }

    /// Whether this provider supports the given reasoning-effort id. Default
    /// checks the declared `reasoning_efforts()` set — capability, not id.
    fn supports_reasoning_effort(&self, effort: &str) -> bool {
        self.reasoning_efforts().iter().any(|e| e.id == effort)
    }

    /// Structurally validate a requested reasoning effort against this provider's
    /// capability. `None`/empty is always valid (no selection). An unsupported
    /// value yields a `ReasoningEffortRejection` carrying the supported set —
    /// callers reject rather than silently downgrade.
    fn validate_reasoning_effort(
        &self,
        effort: Option<&str>,
    ) -> Result<(), ReasoningEffortRejection> {
        match effort {
            None => Ok(()),
            Some("") => Ok(()),
            Some(e) if self.supports_reasoning_effort(e) => Ok(()),
            Some(e) => Err(ReasoningEffortRejection {
                provider: self.id().to_string(),
                requested: e.to_string(),
                supported: self.reasoning_efforts().into_iter().map(|r| r.id).collect(),
            }),
        }
    }

    /// In-agent command that triggers context compaction, if supported.
    fn compact_command(&self) -> Option<&str> {
        None
    }

    /// In-agent command that starts a fresh context in the same tab, if supported.
    fn fresh_context_command(&self) -> Option<&str> {
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
    /// Returning `None` means the provider has no in-tab stop hook
    /// and the UI should hide the Stop affordance for that tab.
    fn stop_keystroke(&self) -> Option<&str> {
        None
    }

    // ── Plugin skill delivery ────────────────────────────────────────

    /// Install a plugin-contributed skill file for this agent. Providers
    /// that can't carry skills (system-prompt-only agents) ignore this.
    fn install_skills(&self, _skills: &[crate::plugin_manager::SkillFile]) -> Result<(), String> {
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

    /// Query the models this provider currently offers, live.
    /// None means "can't probe; use the manifest seed."
    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        None
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

    /// Per-platform install command run inside a fresh terminal tab when the
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
            reasoning_efforts: self.reasoning_efforts(),
            default_reasoning_effort: self.default_reasoning_effort(),
            settings_schema: settings,
            notifications_schema: self.notifications_schema(),
            supports_lazy_skills: self.supports_lazy_skills(),
            has_hooks: self.has_hooks(),
            cli_binary: self.cli_binary().map(|s| s.to_string()),
            install_command: self.install_command(platform),
            model_switch_format: self.model_switch_format().map(|s| s.to_string()),
            fresh_context_command: self.fresh_context_command().map(|s| s.to_string()),
            compact_command: self.compact_command().map(|s| s.to_string()),
            supports_stop: self.stop_keystroke().is_some(),
            supports_chat_ui: self.supports_chat_ui(),
            supports_structured_prompts: self.supports_structured_prompts(),
            supports_watcher: false,
            trust: self.trust(),
            transport: ProviderTransport::Pty,
            behavior_config: self.behavior_config(),
            presets: Vec::new(),
            system_prompt_delivery: self.system_prompt_delivery(),
        }
    }

    /// Behavior surface exposed for configuration. PTY agents bring their own
    /// prompt set by default.
    fn behavior_config(&self) -> BehaviorConfig {
        BehaviorConfig::OwnPromptSet
    }

    /// How a system prompt reaches this provider. Default `External` — PTY
    /// agents carry their own prompts and take no system-prompt flag here.
    fn system_prompt_delivery(&self) -> SystemPromptDelivery {
        SystemPromptDelivery::External
    }

    /// Whether a tab running this provider can be shown as chat. Defaults
    /// to "yes if it serves structured chat"; JSONL providers override to
    /// `true`. The frontend reads this off the descriptor — no hardcoded
    /// provider list.
    fn supports_chat_ui(&self) -> bool {
        self.uses_structured_chat()
    }

    /// Whether this provider, in its CURRENT interactive run mode, exposes a
    /// STRUCTURED source for pending prompts (permission / question pickers)
    /// instead of leaving them only as ephemeral PTY terminal UI. When true,
    /// core builds `tab.pending_prompt` from that structured handle and only
    /// falls back to the native attributed-grid PTY detector when the handle
    /// yields nothing. Capability-gated — core never branches on provider id.
    ///
    /// Examples: claude exposes its AskUserQuestion in the parsed JSONL
    /// transcript (`parse_interactive_events`); opencode exposes a local SSE
    /// event bus + reply endpoints. codex app-server and cursor ACP are NOT
    /// reachable in interactive-PTY mode, so they stay on the native detector.
    fn supports_structured_prompts(&self) -> bool {
        false
    }

    /// Build the STRUCTURED pending prompt for an interactive tab, if one is
    /// active and unanswered, from this provider's structured source. `cwd` /
    /// `session_id` identify the tab's own session (detection keys off the
    /// tab's OWN session, never "latest global"). Pure + I/O-only (no locks,
    /// no process calls) — same contract as `parse_session_delta`. Returns
    /// `None` when there is no active structured prompt; core then falls back
    /// to the native PTY detector. Only ever invoked when
    /// `supports_structured_prompts()` is true.
    fn structured_pending_prompt(
        &self,
        _cwd: &str,
        _session_id: &str,
    ) -> Option<codeterm_types::pty::PendingPrompt> {
        None
    }

    /// Whether this provider's structured handle is a local SERVER the host must
    /// bind: the host allocates a fixed `127.0.0.1:port`, passes it at launch
    /// (`AgentLaunchParams::structured_prompt_server`), and subscribes to it.
    /// True for opencode (SSE bus); false for pull-handle providers like claude
    /// that read their own on-disk transcript. Implies
    /// `supports_structured_prompts`.
    fn needs_structured_prompt_server(&self) -> bool {
        false
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
