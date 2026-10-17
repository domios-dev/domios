macro_rules! agent_provider_model_behavior {
    () => {
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

        fn reasoning_effort_launch_args_for_model(
            &self,
            _model: Option<&str>,
            effort: &str,
        ) -> Vec<String> {
            self.reasoning_effort_launch_args(effort)
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

        /// How a live tab switches model. An in-agent command is never offered:
        /// provider slash commands persist the choice as the machine-global default.
        fn model_switch(&self) -> ModelSwitchMode {
            ModelSwitchMode::Unsupported
        }

        fn model_switch_commands(&self) -> Option<ModelSwitchCommands> {
            None
        }

        /// In-agent command that exits the REPL cleanly, if supported.
        fn exit_command(&self) -> Option<&str> {
            None
        }

        fn supports_idle_restart_termination(&self) -> bool {
            false
        }

        /// Raw key bytes from a provider's single-key `stopKey` declaration.
        /// Returning `None` means the provider has no in-tab stop hook
        /// and the UI should hide the Stop affordance for that tab.
        fn stop_keystroke(&self) -> Option<&str> {
            None
        }

        fn uses_win32_control_keys(&self) -> bool {
            false
        }

        /// Ordered raw key values from a provider's stop declaration.
        fn stop_keys(&self) -> Option<Vec<String>> {
            self.stop_keystroke().map(|key| vec![key.to_string()])
        }

        /// Whether a later key waits for a PTY screen-change event.
        fn stop_key_wait_for_output(&self) -> bool {
            false
        }

        // ── Plugin skill delivery ────────────────────────────────────────
    };
}
