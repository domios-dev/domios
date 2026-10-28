macro_rules! agent_provider_session_usage {
    () => {
        /// Build the path to the session file for this agent.
        fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf>;

        fn session_model_identity_path(&self, _cwd: &str, _session_id: &str) -> Option<PathBuf> {
            None
        }

        /// Further usage logs owned by this exact session, found beside `session_file`
        /// (e.g. sub-agent transcripts). Never another session's; empty when none.
        fn additional_usage_sources(
            &self,
            _session_file: &Path,
            _session_id: &str,
        ) -> Vec<PathBuf> {
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

        /// Some(false) is confirmed absence; None is an unreadable or unknown store.
        fn session_exists(&self, cwd: &str, session_id: &str) -> Option<bool> {
            self.session_file_path(cwd, session_id)?.try_exists().ok()
        }

        /// Creation evidence for sessions without a per-session file.
        fn session_created_at(
            &self,
            _cwd: &str,
            _session_id: &str,
        ) -> Option<std::time::SystemTime> {
            None
        }

        /// Build a command to resume an existing session.
        fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String>;

        fn supports_exact_session_resume(&self) -> bool {
            false
        }

        /// Can resume a session while replacing its initial instructions.
        fn supports_initial_prompt_update_on_resume(&self) -> bool {
            false
        }

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
    };
}
