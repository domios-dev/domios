use super::{wire::SerdeSessionMessage, JsProvider};
use crate::{jsonl_window::records::read_records, provider::SessionMessage};
use serde_json::json;
use std::path::Path;

use super::wire::SESSION_PARSE_CHUNK_BYTES;
use std::io::{BufRead, BufReader};

impl JsProvider {
    pub(super) fn js_parse_session_messages(&self, file_path: &Path) -> Vec<SessionMessage> {
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
                None => Vec::new(),
            };
        }
        let Ok(file) = std::fs::File::open(file_path) else {
            return Vec::new();
        };
        let mut reader = BufReader::new(file);
        let mut out = Vec::new();
        let mut chunk = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            chunk.push_str(&line);
            if chunk.len() >= SESSION_PARSE_CHUNK_BYTES {
                self.extend_parsed_session_chunk(&mut out, &chunk);
                chunk.clear();
            }
        }
        if !chunk.is_empty() {
            self.extend_parsed_session_chunk(&mut out, &chunk);
        }
        out
    }

    pub(super) fn js_parse_session_message_records(&self, path: &Path) -> Vec<SessionMessage> {
        let Some(source) = self
            .manifest
            .capabilities
            .record_sources
            .get("sessionTranscript")
        else {
            return Vec::new();
        };
        let mut offset = 0;
        let mut messages = Vec::new();
        loop {
            let Ok(window) = read_records(
                path,
                offset,
                super::wire::SESSION_PARSE_CHUNK_BYTES as u64,
                source,
                None,
            ) else {
                break;
            };
            if window.meta.new_offset <= offset || window.projection_error.is_some() {
                break;
            }
            let Some(rows) = self.try_call_records::<Vec<SerdeSessionMessage>>(
                "parseSessionMessageRecords",
                &[json!(window.records), json!(window.meta)],
                &window,
            ) else {
                break;
            };
            messages.extend(rows.into_iter().map(SessionMessage::from));
            offset = window.meta.new_offset;
        }
        messages
    }
}
