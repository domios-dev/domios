macro_rules! agent_provider_session_content {
    () => {
        /// What kind of session-id control this provider supports.
        /// Default is `None` — provider generates its own ids and CodeTerm
        /// has to detect them post-launch via `detect_session_id`.
        fn session_id_injection(&self) -> SessionIdInjection {
            SessionIdInjection::None
        }

        fn attachment_capabilities(&self) -> AttachmentCapabilities {
            AttachmentCapabilities::default()
        }

        fn context_attach(&self) -> Option<ContextAttach> {
            None
        }

        fn is_tool_less(&self) -> bool {
            false
        }

        fn supports_reply_relay(&self) -> bool {
            false
        }

        fn read_reply_relay_turn(&self, _: &str, _: &str, _: u64) -> Option<ReplyRelayEvidence> {
            None
        }

        fn supports_launch_session_detection(&self) -> bool {
            false
        }

        /// Session named by this launch's own provider record; `None` when no record
        /// names it. Never a cwd or recency guess.
        fn detect_launch_session(
            &self,
            _: &LaunchSessionEvidence,
        ) -> Option<LaunchSessionDetection> {
            None
        }

        /// Enumerate all past sessions this provider keeps on disk.
        /// Used by the cross-provider session search. Default returns empty —
        /// providers without on-disk session history (or not yet implemented)
        /// contribute nothing and fall out of search silently.
        fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
            Vec::new()
        }

        /// Fast-path session metadata used by content search.
        fn enumerate_sessions_for_search(&self) -> Vec<SessionMetadata> {
            self.enumerate_sessions()
        }

        /// Resolve optional session fields after content search selects a candidate.
        fn describe_session_for_search(&self, session: &SessionMetadata) -> SessionMetadata {
            session.clone()
        }

        /// This provider's own record for exactly `session_id`; `None` when it does
        /// not own the id. Never walks session history, never guesses by cwd or recency.
        fn find_session(&self, _session_id: &str) -> Option<SessionMetadata> {
            None
        }

        /// Pure, I/O-only session content search projection.
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

        /// This session's JSONL path, or None for a different storage capability.
        fn session_jsonl_path(&self, _cwd: &str, _session_id: &str) -> Option<std::path::PathBuf> {
            None
        }

        /// Hold incomplete trailing records at their start byte for the next read.
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

        /// Host-bounded parsing; unsupported parsers must not silently read beyond the budget.
        fn parse_session_delta_bounded(
            &self,
            _path: &std::path::Path,
            _from_offset: u64,
            _byte_budget: u64,
        ) -> Option<codeterm_types::agent::chat_types::SessionDelta> {
            None
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

        fn structured_tool_file_diffs(
            &self,
            _part: &serde_json::Value,
        ) -> Option<serde_json::Value> {
            None
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
    };
}
