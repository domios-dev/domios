use super::*;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Trait for agent backend providers (Claude, Codex, Aider, etc.)
#[async_trait]
pub trait AgentProvider: Send + Sync {
    /// Unique identifier for this provider (e.g. "claude").
    fn id(&self) -> &str;

    /// Human-readable name (e.g. "Claude Code").
    fn display_name(&self) -> &str;

    /// Executable name a user would type to launch this provider from a shell
    /// (e.g. `claude`, `codex`, `opencode`). Used by the input parser to map
    /// typed commands → agent_type and to preflight launches. Defaults to `id()`.
    fn binary_name(&self) -> &str {
        self.id()
    }

    /// Maximum text body size accepted as an inline paste. `None` means undeclared.
    fn paste_limit_bytes(&self) -> Option<usize> {
        None
    }

    /// CLI flag accepting one extra writable directory in sandboxed worktree launches.
    fn additional_writable_dir_flag(&self) -> Option<String> {
        None
    }

    fn sandboxed_linux_launch_capability(&self) -> Option<SandboxedLinuxLaunchCapability> {
        None
    }

    /// What THIS machine says about the launch binary, through the shared
    /// resolver. Core reads this instead of asking a provider to self-report.
    fn observe_launch_binary(&self) -> LaunchBinaryObservation {
        super::availability::observe_launch_binary(Some(self.binary_name()))
    }

    /// Build the shell command to launch this agent.
    fn build_launch_command(&self, params: &AgentLaunchParams) -> String;

    fn release_tab_launch_artifacts(
        &self,
        _tab_id: &str,
        _instance_id: &str,
    ) -> Result<bool, String> {
        Ok(false)
    }

    fn folder_trust_answered(&self, _tab_id: &str, _instance_id: &str) -> Result<bool, String> {
        Ok(false)
    }

    fn sweep_launch_artifacts(
        &self,
        _request: &LaunchArtifactSweepRequest,
    ) -> Result<Option<LaunchArtifactSweepReport>, String> {
        Ok(None)
    }

    fn starter_prompt_text(&self, _: StarterPrompt) -> Option<String> {
        None
    }

    /// How to wait for the agent to be ready after PTY spawn.
    fn readiness_strategy(&self) -> ReadinessStrategy;

    /// How to deliver the task to the agent.
    fn task_delivery(&self) -> TaskDelivery;

    fn provider_readiness_strategy(&self) -> ProviderReadinessStrategy {
        ProviderReadinessStrategy::default()
    }

    /// Declared terminal placement of the live composer cursor.
    fn composer_cursor_position(&self) -> ComposerCursorPosition {
        ComposerCursorPosition::default()
    }

    fn session_starts_on_first_user_turn(&self) -> bool {
        false
    }

    fn public_send_waits_for_startup_turn_completion(&self) -> bool {
        false
    }

    /// How to verify that submitted text was actually processed. Default is
    /// shell-prompt polling — providers running TUIs or non-observable flows
    /// override (TuiInputDrained / None).
    fn verify_submit_strategy(&self) -> VerifySubmitStrategy {
        VerifySubmitStrategy::ShellPrompt {
            timeout: Duration::from_secs(5),
        }
    }

    fn turn_evidence_normalization(&self) -> TurnEvidenceNormalization {
        TurnEvidenceNormalization::default()
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

    /// Further usage logs owned by this exact session, found beside `session_file`
    /// (e.g. sub-agent transcripts). Never another session's; empty when none.
    fn additional_usage_sources(&self, _session_file: &Path, _session_id: &str) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Every usage log this provider owns for exactly this session, each scoped to
    /// the session whose rows it holds. Defaults to the session file plus the
    /// provider's additional sources; a provider whose usage lives in a log shared
    /// by many sessions (e.g. one sqlite database) overrides this with a
    /// session-scoped source instead of a fabricated per-session file.
    fn usage_sources(&self, cwd: &str, session_id: &str) -> Vec<UsageSourceSpec> {
        super::usage_sources_from_session_file(self, cwd, session_id)
    }

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

    /// Resume command carrying the full launch context: trailing `args` plus
    /// `system_prompt` re-applied through this provider's own system-prompt
    /// channel. Overriding declares "my resume drops the launch-time prompt";
    /// the default declares the opposite, so an undeclared provider keeps its
    /// retained copy and is never double-injected.
    fn build_resume_command_with_context(&self, params: &AgentResumeParams) -> Option<String> {
        let base = self.build_resume_command(&params.session_id, params.skip_permissions)?;
        Some(append_launch_args(base, &params.args))
    }

    /// Cumulative token usage from the provider's session log.
    fn read_usage(&self, _cwd: &str, _session_id: &str) -> Option<UsageSnapshot> {
        None
    }
    fn usage_parser(&self) -> Option<&dyn crate::usage_parser::UsageParser> {
        None
    }

    fn provider_allowance_capability(&self) -> Option<ProviderAllowanceCapability> {
        self.usage_parser()
            .is_some()
            .then_some(ProviderAllowanceCapability {
                default_reserve_pct: 5.0,
                telemetry_max_age_ms: 15 * 60 * 1_000,
            })
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

    /// Session named by this launch's own provider record; `None` when no record
    /// names it. Never a cwd or recency guess.
    fn detect_launch_session(&self, _: &LaunchSessionEvidence) -> Option<LaunchSessionDetection> {
        None
    }

    /// Enumerate all past sessions this provider keeps on disk.
    /// Used by the cross-provider session search. Default returns empty —
    /// providers without on-disk session history (or not yet implemented)
    /// contribute nothing and fall out of search silently.
    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        Vec::new()
    }

    /// This provider's own record for exactly `session_id`; `None` when it does
    /// not own the id. Never walks session history, never guesses by cwd or recency.
    fn find_session(&self, _session_id: &str) -> Option<SessionMetadata> {
        None
    }

    /// Extract conversation messages from a session file produced by this
    /// provider. Called by the search pipeline for each candidate session;
    /// returning `vec![]` means this provider can't participate in content
    /// search yet. Must be pure + I/O-only (no locks, no process calls).
    fn parse_session_messages(&self, file_path: &std::path::Path) -> Vec<SessionMessage> {
        let _ = file_path;
        Vec::new()
    }

    /// Where content search reads this provider's session text from.
    fn session_content_source(&self) -> SessionContentSource {
        SessionContentSource::None
    }

    /// One session's messages from a `SharedStore`, scanning at most
    /// `query.max_bytes`. `None` means the store could not be read. Pure + I/O-only.
    fn read_session_content(
        &self,
        _session_id: &str,
        _query: &SessionContentQuery,
    ) -> Option<SessionContent> {
        None
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

    /// Declared hook event roster: (event name → ingestion route) pairs core
    /// registers verbatim — an undeclared event routes nowhere. Default:
    /// hooked providers post the SessionStart binding event; hookless none.
    fn hook_event_roster(&self) -> Vec<HookEventBinding> {
        if self.has_hooks() {
            session_start_hook_roster()
        } else {
            Vec::new()
        }
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

    /// Recognize this provider's directory-trust dialog on the live screen.
    /// Core sends a key only when `is_directory_trusted` authorizes the exact cwd.
    fn directory_trust_prompt_response(
        &self,
        _screen: &str,
    ) -> Option<DirectoryTrustPromptResponse> {
        None
    }

    fn is_directory_trusted(&self, _path: &str) -> Result<bool, String> {
        Ok(false)
    }

    fn directory_trust_state(
        &self,
        _path: &str,
        _skip_permissions: bool,
    ) -> Option<crate::provider::DirectoryTrustState> {
        None
    }

    /// One next key for a provider-declared, safe launch onboarding choice.
    /// The host re-observes the screen before requesting another key.
    fn launch_onboarding_response(&self, _screen: &str) -> Option<ProviderPromptKeyStep> {
        None
    }

    fn parse_directory_trust_prompt(
        &self,
        _screen: &str,
    ) -> Option<codeterm_types::pty::PendingPrompt> {
        None
    }

    /// Recognize this provider's tool-permission prompt for the hook-reported
    /// `tool_name` and name its affirmative option. `None` leaves it to the user.
    fn permission_prompt_response(
        &self,
        _tool_name: &str,
        _prompt: &codeterm_types::pty::PendingPrompt,
        _screen: &str,
    ) -> Option<crate::permission_prompt::PermissionPromptResponse> {
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

    /// Runtime-confirmed reasoning effort from this exact claimed session.
    /// Providers must not consult a global/latest record when implementing it.
    fn detect_session_reasoning_effort(&self, _cwd: &str, _session_id: &str) -> Option<String> {
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

    fn reasoning_efforts_for_model(&self, model: Option<&str>) -> Vec<ReasoningEffortInfo> {
        model
            .and_then(|model_id| {
                self.available_models()
                    .into_iter()
                    .find(|model| model.id.eq_ignore_ascii_case(model_id))
                    .and_then(|model| model.reasoning_efforts)
            })
            .unwrap_or_else(|| self.reasoning_efforts())
    }

    fn supports_reasoning_effort_for_model(&self, model: Option<&str>, effort: &str) -> bool {
        self.reasoning_efforts_for_model(model)
            .iter()
            .any(|supported| supported.id == effort)
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
    /// agent's current generation without exiting — the provider's
    /// declared stopKey (Esc for all bundled providers).
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

    // Providers can integrate through hook configuration, managed settings, or marked files.

    /// Status of CodeTerm's notification integration with this provider —
    /// returned as opaque JSON so each provider shapes it for its own UI.
    /// Default: providers without external integration report `status: "ok"`
    /// and the Badge renders "available".
    fn check_integration(&self) -> serde_json::Value {
        serde_json::json!({ "status": "ok" })
    }

    /// The integration surface this provider owns and may reconcile.
    fn integration_kind(&self) -> IntegrationKind {
        IntegrationKind::None
    }

    /// Startup repair is capability-gated so core never identifies providers.
    fn has_installable_integration(&self) -> bool {
        self.integration_kind() != IntegrationKind::None
    }

    /// Install CodeTerm's provider integration. `api_port` targets local callback
    /// routes when the integration uses hooks. Default no-op.
    fn install_integration(
        &self,
        _api_port: u16,
        options: &IntegrationInstallOptions,
    ) -> Result<(), String> {
        if !options.consent {
            return Err(
                "provider integration installation requires explicit local-user consent".into(),
            );
        }
        Ok(())
    }

    fn uninstall_integration(&self) -> Result<(), String> {
        Ok(())
    }

    fn restore_integration(&self) -> Result<(), String> {
        Err("provider integration has no recoverable backup".into())
    }

    /// Optional `integration.trust_grant` declaration; default `None` — the
    /// provider's hooks need no trust grant.
    fn integration_trust_grant_capability(&self) -> Option<IntegrationTrustGrantCapability> {
        None
    }

    /// Provider-owned hook-trust grant scoped to the declared managed key(s).
    /// The provider CLI is the only hash authority; failure degrades honestly.
    fn grant_integration_trust(&self, _managed_keys: &[String]) -> serde_json::Value {
        serde_json::json!({ "granted": false, "degraded": true, "reason": "provider declares no trust grant capability" })
    }

    /// Retained for provider-owned read-only startup checks. Core owns consented
    /// contract repair so it can preserve consent and avoid trust re-grants.
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

    fn model_catalogue_capability(&self) -> ModelCatalogueCapability {
        ModelCatalogueCapability::default()
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

    /// Provider-owned groupings of its models, declared as pure data (no I/O):
    /// the picker renders one section per group and core carries the labels
    /// through untouched. Empty ⇒ one flat list.
    fn model_groups(&self) -> Vec<ModelGroup> {
        Vec::new()
    }

    /// Cumulative usage per model group, from whatever source the provider
    /// declares. May do I/O, so it is never called from `describe()` — only
    /// from the on-demand picker command. Empty ⇒ the provider publishes no
    /// per-group usage and the picker shows none.
    fn model_group_usage(&self) -> Vec<ModelGroupUsage> {
        Vec::new()
    }

    /// Cheap, pure predicate: this provider publishes per-group usage even
    /// without a usage parser. It drives the auto-injected Usage Tracking
    /// toggle, so a group-spend capsule is gated like any other usage surface.
    fn reports_group_usage(&self) -> bool {
        false
    }

    /// Files whose change means this provider's credentialed model availability
    /// changed. The catalog fingerprints them and re-probes when one moves, so
    /// connecting a provider surfaces its models without a rebuild. Empty ⇒ the
    /// provider's catalog does not depend on an observable credential file.
    fn model_credential_files(&self) -> Vec<std::path::PathBuf> {
        Vec::new()
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

    fn update_command(&self, _platform: &str) -> Option<String> {
        None
    }

    fn supports_update_check(&self) -> bool {
        false
    }

    fn check_for_update(&self) -> Option<ProviderUpdateAvailability> {
        None
    }

    /// Short note picker surfaces show beside this provider's name, such as a
    /// free-tier offer. `None` means the provider makes no such claim.
    fn badge(&self) -> Option<&str> {
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
    /// **Contract:** this method MUST be pure, cheap, and free of I/O.
    /// No locks, no `.await`, no remote calls. It is invoked while
    /// holding a read lock on the agent registry, and the result is
    /// serialized to JSON for REST/Socket/Tauri callers. The default
    /// impl just assembles the other metadata methods.
    fn describe(&self) -> ProviderDescriptor {
        let platform = current_platform_tag();
        let mut settings = self.settings_schema();
        if self.usage_parser().is_some() {
            let probe_cmd = self.usage_probe_command().unwrap_or("/usage");
            settings.push(usage_tracking_section(probe_cmd));
        } else if self.reports_group_usage() {
            settings.push(usage_store_tracking_section());
        }
        ProviderDescriptor {
            schema_version: PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
            id: self.id().to_string(),
            display_name: self.display_name().to_string(),
            icon_hint: self.icon_hint().map(|s| s.to_string()),
            icon_svg: self.icon_svg().map(|s| s.to_string()),
            icon_color: self.icon_color().map(|s| s.to_string()),
            // Keep unavailable entries: `available` greys a model out in the
            // picker, it must not vanish it. Matches chatbackend describe().
            available_models: self.available_models(),
            model_catalogue: self.model_catalogue_capability(),
            model_groups: self.model_groups(),
            default_model: self.default_model(),
            reasoning_efforts: self.reasoning_efforts(),
            default_reasoning_effort: self.default_reasoning_effort(),
            settings_schema: settings,
            notifications_schema: self.notifications_schema(),
            supports_lazy_skills: self.supports_lazy_skills(),
            has_hooks: self.has_hooks(),
            supports_permission_bypass: !self.auto_approve_flags().is_empty(),
            cli_binary: self.cli_binary().map(|s| s.to_string()),
            install_command: self.install_command(platform),
            badge: self.badge().map(|s| s.to_string()),
            update_command: self.update_command(platform),
            model_switch_format: self.model_switch_format().map(|s| s.to_string()),
            fresh_context_command: self.fresh_context_command().map(|s| s.to_string()),
            compact_command: self.compact_command().map(|s| s.to_string()),
            integration_trust_grant: self.integration_trust_grant_capability(),
            supports_stop: self.stop_keystroke().is_some(),
            supports_chat_ui: self.supports_chat_ui(),
            supports_structured_prompts: self.supports_structured_prompts(),
            supports_permission_auto_confirm: self.supports_permission_auto_confirm(),
            supports_watcher: false,
            paste_limit_bytes: self.paste_limit_bytes(),
            trust: self.trust(),
            transport: ProviderTransport::Pty,
            behavior_config: self.behavior_config(),
            presets: Vec::new(),
            system_prompt_delivery: self.system_prompt_delivery(),
            context_owner: ContextOwner::Plugin,
            availability: ProviderAvailability::default(),
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

    /// Whether a dedicated directory-trust parser may run after a cheap screen precheck.
    fn supports_directory_trust_prompt_parsing(&self) -> bool {
        false
    }

    /// Whether `permission_prompt_response` can recognize any prompt, so the UI
    /// offers per-tab auto-confirm only where it can ever answer.
    fn supports_permission_auto_confirm(&self) -> bool {
        false
    }

    fn supports_classified_prompt_auto_response(&self) -> bool {
        false
    }

    fn classified_prompt_auto_response(
        &self,
        _state: crate::tab_state::TabState,
        _screen: &str,
    ) -> Option<crate::provider::ClassifiedPromptOptionRole> {
        None
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
