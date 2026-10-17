use super::*;

impl JsProvider {
    pub(super) fn js_runtime_stored_history_available(&self, cwd: &str, session_id: &str) -> bool {
        self.agent_runtime()
            .is_some_and(|runtime| runtime.stored_history)
            && (!self.has_js("runtimeStoredHistoryAvailable")
                || self
                    .try_call(
                        "runtimeStoredHistoryAvailable",
                        &[json!(cwd), json!(session_id)],
                    )
                    .unwrap_or(false))
    }

    pub(super) fn js_session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        // Hook-reported evidence wins over any reconstruction; absence falls through.
        let path = crate::session_evidence::stored_transcript_path(session_id)
            .or_else(|| self.owned_session_file_path(cwd, session_id))?;
        if self
            .manifest
            .commands
            .model_switch_commands
            .as_ref()
            .is_some_and(|commands| commands.turn_completion.is_some())
        {
            self.permitted_session_path(path)
        } else {
            Some(path)
        }
    }

    pub(super) fn js_runtime_stored_history(
        &self,
        session_id: &str,
        records: &str,
    ) -> Result<Vec<codeterm_types::agent::chat_types::ChatMsg>, String> {
        if !self
            .agent_runtime()
            .is_some_and(|capability| capability.stored_history)
        {
            return Err("Provider has no native stored runtime history projection".into());
        }
        if let Some(source) = self
            .manifest
            .capabilities
            .record_sources
            .get("storedHistory")
            .filter(|_| self.has_js("runtimeStoredRecords"))
        {
            let window = crate::jsonl_window::records::parse_records_with_limit(
                records.as_bytes(),
                0,
                source,
                Some(session_id),
                1024 * 1024,
            );
            return self.js_runtime_stored_records(session_id, &window);
        }
        self.try_call("runtimeStoredHistory", &[json!(session_id), json!(records)])
            .ok_or_else(|| "Stored runtime history projection failed".into())
    }

    pub(super) fn js_runtime_stored_records(
        &self,
        session_id: &str,
        window: &crate::jsonl_window::records::RecordWindow,
    ) -> Result<Vec<codeterm_types::agent::chat_types::ChatMsg>, String> {
        if let Some(error) = &window.projection_error {
            return Err(error.clone());
        }
        self.try_call_records(
            "runtimeStoredRecords",
            &[json!(session_id), json!(window.records), json!(window.meta)],
            window,
        )
        .ok_or_else(|| "Stored runtime record projection failed".into())
    }

    pub(super) fn js_structured_tool_file_diffs(
        &self,
        part: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        if self.has_js("structuredToolFileDiffs") {
            return self
                .try_call("structuredToolFileDiffs", &[part.clone()])
                .flatten();
        }
        self.native()
            .and_then(|n| n.structured_tool_file_diffs(part))
    }

    pub(super) fn js_session_jsonl_path(
        &self,
        cwd: &str,
        session_id: &str,
    ) -> Option<std::path::PathBuf> {
        let path = self.session_jsonl_path_unchecked(cwd, session_id)?;
        if self
            .manifest
            .commands
            .model_switch_commands
            .as_ref()
            .is_some_and(|commands| commands.turn_completion.is_some())
        {
            self.permitted_session_path(path)
        } else {
            Some(path)
        }
    }

    fn session_jsonl_path_unchecked(
        &self,
        cwd: &str,
        session_id: &str,
    ) -> Option<std::path::PathBuf> {
        // Hook-reported evidence wins over any reconstruction; absence falls through.
        if let Some(stored) = crate::session_evidence::stored_transcript_path(session_id) {
            return Some(stored);
        }
        if !self.has_js("sessionJsonlPath") {
            return self
                .native()
                .and_then(|n| n.session_jsonl_path(cwd, session_id));
        }
        let resolved = self.session_cwd(cwd);
        self.try_call::<Option<String>>(
            "sessionJsonlPath",
            &[serde_json::json!(resolved), serde_json::json!(session_id)],
        )
        .flatten()
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
    }

    pub(super) fn session_cwd(&self, cwd: &str) -> String {
        match self.manifest.sessions.as_ref().map(|s| s.cwd_encoding) {
            Some(super::super::CwdEncoding::DashEncoded) => codeterm_paths::canonical_cwd(cwd),
            _ => cwd.to_string(),
        }
    }

    pub(super) fn js_read_structured_chat(
        &self,
        cwd: &str,
        session_id: &str,
        cursor: Option<&str>,
    ) -> codeterm_types::agent::chat_types::StructuredChat {
        if self.has_js("readStructuredChat") {
            let result = self.try_call::<codeterm_types::agent::chat_types::StructuredChat>(
                "readStructuredChat",
                &[json!(cwd), json!(session_id), json!(cursor)],
            );
            log::debug!(
                "[session-turn-evidence] plugin-read session={} input_cursor={:?} decoded={} rows={} output_cursor={:?}",
                session_id,
                cursor,
                result.is_some(),
                result.as_ref().map_or(0, |chat| chat.messages.len()),
                result.as_ref().and_then(|chat| chat.cursor.as_deref()),
            );
            return result.unwrap_or_default();
        }
        match self.native() {
            Some(n) => n.read_structured_chat(cwd, session_id, cursor),
            None => codeterm_types::agent::chat_types::StructuredChat::default(),
        }
    }

    pub(super) fn js_parse_interactive_events(
        &self,
        msg: &codeterm_types::agent::chat_types::RichMessage,
    ) -> Vec<codeterm_types::agent::chat_types::InteractiveElement> {
        if self.manifest.spawn.interactive_events && self.has_js("parseInteractiveEvents") {
            let arg = match serde_json::to_value(msg) {
                Ok(v) => v,
                Err(_) => return Vec::new(),
            };
            return self
                .try_call::<Vec<codeterm_types::agent::chat_types::InteractiveElement>>(
                    "parseInteractiveEvents",
                    &[arg],
                )
                .unwrap_or_default();
        }
        self.native()
            .map(|n| n.parse_interactive_events(msg))
            .unwrap_or_default()
    }
}
