macro_rules! agent_provider_launch_readiness {
    () => {
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

        /// Bounded input fallback: when the provider's own readiness signal never
        /// fires, open input at this deadline instead of blocking the tab forever.
        fn input_fallback_ms(&self) -> Option<u64> {
            None
        }

        /// When a provider-declared startup dialog is auto-answered during launch.
        /// Default: delegated roles only (Worker / planner Manager).
        fn onboarding_auto_answer(&self) -> OnboardingAutoAnswer {
            OnboardingAutoAnswer::default()
        }

        fn restores_conversation_history_on_resume(&self) -> bool {
            false
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

        /// Whether this provider permits recovery keys without an authenticated session.
        fn supports_unbound_submit_recovery(&self) -> bool {
            false
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
    };
}
