macro_rules! agent_provider_runtime_defaults {
    () => {
        fn cli_login(&self) -> Option<CliLoginCapability> {
            None
        }
        fn capability_probe(&self) -> Option<CapabilityProbe> {
            None
        }
        fn auth_status_probe(&self) -> Option<AuthStatusProbe> {
            None
        }
        fn tracks_background_work(&self) -> bool {
            false
        }
        fn fork_session_args(&self) -> Option<Vec<String>> {
            None
        }
        fn parse_background_work(
            &self,
            _record: &serde_json::Value,
        ) -> Vec<codeterm_types::agent::background_work::BackgroundWorkObservation> {
            Vec::new()
        }

        fn parse_auth_status(&self, _record: &serde_json::Value) -> Option<AuthStatus> {
            None
        }

        fn parse_session_observations(
            &self,
            _record: &serde_json::Value,
        ) -> Option<SessionObservations> {
            None
        }
        fn remote_task_capability(&self) -> Option<crate::remote_tasks::RemoteTaskCapability> {
            None
        }

        fn agent_runtime(&self) -> Option<crate::agent_runtime::AgentRuntimeCapability> {
            None
        }

        fn runtime_stored_history_available(&self, _cwd: &str, _session_id: &str) -> bool {
            self.agent_runtime()
                .is_some_and(|runtime| runtime.stored_history)
        }

        fn runtime_stored_history(
            &self,
            _session_id: &str,
            _records: &str,
        ) -> Result<Vec<codeterm_types::agent::chat_types::ChatMsg>, String> {
            Err("Provider has no native stored runtime history projection".into())
        }

        fn record_source(
            &self,
            _source: &str,
        ) -> Option<&crate::jsonl_window::records::RecordSource> {
            None
        }

        fn parse_session_records(
            &self,
            _window: &crate::jsonl_window::records::RecordWindow,
        ) -> Result<codeterm_types::agent::chat_types::SessionDelta, String> {
            Err("Provider has no parsed session record hook".into())
        }

        fn runtime_stored_records(
            &self,
            _session: &str,
            _window: &crate::jsonl_window::records::RecordWindow,
        ) -> Result<Vec<codeterm_types::agent::chat_types::ChatMsg>, String> {
            Err("Provider has no stored record hook".into())
        }

        fn authorize_agent_runtime(
            &self,
            authority: &crate::agent_runtime::LaunchAuthority,
        ) -> Result<PathBuf, crate::js_runtime::manifest::RuntimeManifestError> {
            if !authority.authorizes_provider(self.id()) {
                return Err(crate::js_runtime::manifest::RuntimeManifestError::ConsentRequired);
            }
            self.agent_runtime()
                .ok_or(crate::js_runtime::manifest::RuntimeManifestError::CapabilityRequired)?
                .resolve_executable()
        }
    };
}
