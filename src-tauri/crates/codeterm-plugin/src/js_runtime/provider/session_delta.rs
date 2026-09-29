use codeterm_types::agent::chat_types::SessionDelta;

use super::JsProvider;

impl JsProvider {
    pub(super) fn js_parse_session_delta(
        &self,
        path: &std::path::Path,
        from_offset: u64,
    ) -> SessionDelta {
        // Seek-based and capped: session files reach hundreds of MB, and this runs on every tailer poll.
        let read = crate::jsonl_window::read_records_from(
            path,
            from_offset,
            crate::jsonl_window::DELTA_CHUNK_BYTES,
        );
        let window = match read {
            Ok(w) => w,
            Err(_) => {
                return SessionDelta {
                    new_offset: from_offset,
                    messages: vec![],
                }
            }
        };
        // The reader holds back a partial trailing line, so the plugin never sees it and byte/code-unit mismatches in JS string indexing can't corrupt the offset.
        if window.bytes.is_empty() {
            return SessionDelta {
                new_offset: window.new_offset,
                messages: vec![],
            };
        }
        let consumed_bytes = window.new_offset - from_offset;
        // land inside line bodies, which the plugin's per-line JSON.parse will reject. Byte-count is no longer trusted from JS; we computed it above.
        let chunk = String::from_utf8_lossy(&window.bytes).to_string();
        let context = serde_json::json!({
            "session_key": path.to_string_lossy(),
            "from_offset": from_offset,
        });
        #[derive(serde::Deserialize)]
        struct JsDelta {
            messages: Vec<serde_json::Value>,
        }
        let parsed = self
            .try_call::<JsDelta>("parseSessionDelta", &[serde_json::json!(chunk), context])
            .and_then(|d| crate::jsonl_window::rows_from_plugin(d.messages));
        match parsed {
            Some((mut messages, record_indexes)) => {
                crate::jsonl_window::anchor_rows(
                    &mut messages,
                    &record_indexes,
                    &window.bytes,
                    from_offset,
                );
                SessionDelta {
                    new_offset: from_offset + consumed_bytes,
                    messages,
                }
            }
            None => {
                // messages, but the tailer escapes the infinite-retry trap. The warning from try_call is enough audit trail.
                log::warn!(
                    "[parse_session_delta] plugin '{}' returned no delta for {} bytes at offset {} — skipping window",
                    self.manifest.id, consumed_bytes, from_offset
                );
                SessionDelta {
                    new_offset: from_offset + consumed_bytes,
                    messages: vec![],
                }
            }
        }
    }
}
