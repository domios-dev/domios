use super::{wire::SerdeSessionMessage, JsProvider};
use crate::{jsonl_window::records::read_records, provider::SessionMessage};
use serde_json::json;
use std::path::Path;

use super::wire::SESSION_PARSE_CHUNK_BYTES;
use std::io::{BufRead, BufReader};

impl JsProvider {
    /// Every chunk must project: a failed or interrupted chunk is an error, never a
    /// shorter transcript that callers would read as fewer messages.
    pub(super) fn js_parse_session_messages(
        &self,
        file_path: &Path,
    ) -> Result<Vec<SessionMessage>, String> {
        if self
            .manifest
            .capabilities
            .record_sources
            .contains_key("sessionTranscript")
            && self.has_js("parseSessionMessageRecords")
        {
            return self.js_parse_session_message_records(file_path);
        }
        if !self.has_js("parseSessionMessages") {
            return match self.native() {
                Some(n) => n.parse_session_messages(file_path),
                None => Ok(Vec::new()),
            };
        }
        let file = std::fs::File::open(file_path)
            .map_err(|error| format!("session transcript unreadable: {error}"))?;
        let mut reader = BufReader::new(file);
        let mut out = Vec::new();
        let mut chunk = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            let read = reader
                .read_line(&mut line)
                .map_err(|error| format!("session transcript unreadable: {error}"))?;
            if read == 0 {
                break;
            }
            chunk.push_str(&line);
            if chunk.len() >= SESSION_PARSE_CHUNK_BYTES {
                self.extend_parsed_session_chunk(&mut out, &chunk)?;
                chunk.clear();
            }
        }
        if !chunk.is_empty() {
            self.extend_parsed_session_chunk(&mut out, &chunk)?;
        }
        Ok(out)
    }

    pub(super) fn js_parse_session_message_records(
        &self,
        path: &Path,
    ) -> Result<Vec<SessionMessage>, String> {
        let Some(source) = self
            .manifest
            .capabilities
            .record_sources
            .get("sessionTranscript")
        else {
            return Ok(Vec::new());
        };
        let mut offset = 0;
        let mut messages = Vec::new();
        loop {
            let window = read_records(
                path,
                offset,
                super::wire::SESSION_PARSE_CHUNK_BYTES as u64,
                source,
                None,
            )
            .map_err(|error| format!("session transcript unreadable: {error}"))?;
            if let Some(error) = &window.projection_error {
                return Err(error.clone());
            }
            if window.meta.new_offset <= offset {
                return Ok(messages);
            }
            let rows = super::super::vm::call_records::<Vec<SerdeSessionMessage>>(
                &self.vm,
                "parseSessionMessageRecords",
                &[json!(window.records), json!(window.meta)],
                &window,
            )
            .map_err(|error| format!("parseSessionMessageRecords failed: {error}"))?;
            messages.extend(rows.into_iter().map(SessionMessage::from));
            offset = window.meta.new_offset;
        }
    }
}
