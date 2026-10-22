macro_rules! agent_provider_prompt_detection {
    () => {
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

        fn parse_switch_confirmation_prompt(
            &self,
            _screen: &str,
            _selection: &str,
        ) -> Option<codeterm_types::pty::PendingPrompt> {
            None
        }

        /// Explicit response steps relative to live focus; unsupported or failed hooks abstain.
        fn live_prompt_response(
            &self,
            _option_index: usize,
            _screen: &str,
        ) -> Option<Vec<Vec<u8>>> {
            None
        }

        /// Recognize this provider's directory-trust dialog on the live screen.
        /// Core sends a key only when `is_directory_trusted` authorizes the exact cwd.
        fn directory_trust_prompt_response(
            &self,
            _screen: &str,
        ) -> Option<DirectoryTrustPromptResponse> {
            None
        }

        fn pre_trust_directory(
            &self,
            _path: &str,
        ) -> Result<crate::provider::PreTrustOutcome, String> {
            Ok(crate::provider::PreTrustOutcome::Skipped(
                "not supported".into(),
            ))
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

        /// Exact safe option value for the same narrow onboarding response.
        fn launch_onboarding_safe_choice(&self, _screen: &str) -> Option<String> {
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

        fn supports_session_name_detection(&self) -> bool {
            true
        }

        /// Extract the agent's human-readable session display name from the
        /// current screen — e.g. Claude Code prints "vivid-bhabha-1234" in its
        /// TUI banner. Captured once per agent run and stored on the tab so
        /// `domios tab find <pattern>` can locate tabs by friendly name.
        /// Default returns None; providers that emit a stable handle override.
        fn extract_session_name(&self, _screen: &str) -> Option<String> {
            None
        }

        /// Current model family on the live session — short label only.
        fn detect_session_model(&self, _cwd: &str, _session_id: &str) -> Option<String> {
            None
        }

        fn session_model_transcript_boundary(
            &self,
            _cwd: &str,
            _session_id: &str,
        ) -> Option<codeterm_types::agent::SessionModelTranscriptBoundary> {
            None
        }

        fn detect_session_model_after_boundary(
            &self,
            _cwd: &str,
            _session_id: &str,
            _boundary: &codeterm_types::agent::SessionModelTranscriptBoundary,
        ) -> Option<String> {
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

        fn update_prompt_safe_choice(
            &self,
            _prompt: &codeterm_types::pty::PendingPrompt,
        ) -> Option<String> {
            None
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
    };
}
