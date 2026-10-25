macro_rules! agent_provider_identity {
    () => {
        fn general_agent_offer_when_absent(&self) -> bool {
            false
        }

        fn structured_control(&self) -> Option<StructuredControl> {
            None
        }
        /// Unique identifier for this provider (e.g. "claude").
        fn id(&self) -> &str;

        fn model_provider_config(
            &self,
        ) -> Option<std::sync::Arc<dyn crate::model_providers::ModelProviderConfig>> {
            None
        }

        /// Human-readable name (e.g. "Claude Code").
        fn display_name(&self) -> &str;

        /// Executable name a user would type to launch this provider from a shell
        /// (e.g. `claude`, `codex`, `opencode`). Used by the input parser to map
        /// typed commands → agent_type and to preflight launches. Defaults to `id()`.
        fn binary_name(&self) -> &str {
            self.id()
        }

        fn cli_compatibility(&self) -> Option<crate::provider_compat::CompatibilityManifest> {
            None
        }

        fn worker_lane_settings(&self) -> Vec<crate::provider::WorkerLaneSetting> {
            Vec::new()
        }

        /// Maximum text body size accepted as an inline paste. `None` means undeclared.
        fn paste_limit_bytes(&self) -> Option<usize> {
            None
        }

        fn paste_capability(&self) -> Option<PasteCapability> {
            None
        }

        /// CLI flag accepting one extra writable directory in sandboxed worktree launches.
        fn additional_writable_dir_flag(&self) -> Option<String> {
            None
        }

        fn sandboxed_linux_launch_capability(&self) -> Option<SandboxedLinuxLaunchCapability> {
            None
        }

        fn launch_permission_capability(&self) -> Option<LaunchPermissionCapability> {
            None
        }

        fn execution_sandbox_capability(&self) -> Option<ExecutionSandboxCapability> {
            None
        }

        /// What THIS machine says about the launch binary, through the shared
        /// resolver. Core reads this instead of asking a provider to self-report.
        fn observe_launch_binary(&self) -> LaunchBinaryObservation {
            super::availability::observe_launch_binary(Some(self.binary_name()))
        }

        /// Build the shell command to launch this agent.
        fn build_launch_command(&self, params: &AgentLaunchParams) -> String;

        fn declares_launch_spec(&self) -> bool {
            false
        }

        /// Direct launches skip `preparation::start`; providers returning a spec must not depend on it.
        fn launch_spec(
            &self,
            _params: &crate::provider::LaunchSpecParams<'_>,
        ) -> Option<codeterm_types::agent::LaunchSpec> {
            None
        }

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
    };
}
