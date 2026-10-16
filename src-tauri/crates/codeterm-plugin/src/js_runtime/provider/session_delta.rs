use codeterm_types::agent::chat_types::SessionDelta;

use super::JsProvider;

impl JsProvider {
    pub(super) fn js_parse_session_delta(
        &self,
        path: &std::path::Path,
        from_offset: u64,
        byte_budget: u64,
    ) -> SessionDelta {
        if let Some(source) = self
            .manifest
            .capabilities
            .record_sources
            .get("sessionTranscript")
            .filter(|_| self.has_js("parseSessionRecords"))
        {
            return crate::jsonl_window::records::read_records(
                path,
                from_offset,
                byte_budget,
                source,
                None,
            )
            .ok()
            .and_then(|window| self.js_parse_session_records(&window).ok())
            .unwrap_or(SessionDelta {
                new_offset: from_offset,
                messages: Vec::new(),
            });
        }
        // Seek-based and capped: session files reach hundreds of MB, and this runs on every tailer poll.
        let read = crate::jsonl_window::read_records_from(
            path,
            from_offset,
            byte_budget.min(crate::jsonl_window::DELTA_CHUNK_BYTES),
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
        let line_starts = crate::jsonl_window::record_starts(&window.bytes);
        let chunk = chunk_value(window.bytes);
        let context = serde_json::json!({
            "session_key": path.to_string_lossy(),
            "from_offset": from_offset,
        });
        #[derive(serde::Deserialize)]
        struct JsDelta {
            messages: Vec<serde_json::Value>,
        }
        let parsed = self
            .try_call::<JsDelta>("parseSessionDelta", &[chunk, context])
            .and_then(|d| crate::jsonl_window::rows_from_plugin(d.messages));
        match parsed {
            Some((mut messages, record_indexes)) => {
                crate::jsonl_window::anchor_rows_from_starts(
                    &mut messages,
                    &record_indexes,
                    &line_starts,
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

    pub(super) fn js_parse_session_records(
        &self,
        window: &crate::jsonl_window::records::RecordWindow,
    ) -> Result<SessionDelta, String> {
        if let Some(error) = &window.projection_error {
            return Err(error.clone());
        }
        #[derive(serde::Deserialize)]
        struct Delta {
            messages: Vec<serde_json::Value>,
            new_offset: u64,
        }
        let delta: Delta = self
            .try_call_records(
                "parseSessionRecords",
                &[
                    serde_json::json!(window.records),
                    serde_json::json!(window.meta),
                ],
                window,
            )
            .ok_or("Parsed session record projection failed")?;
        if delta.new_offset != window.meta.new_offset {
            return Err("Parsed session record offset differs from host boundary".into());
        }
        let (mut messages, indexes) = crate::jsonl_window::rows_from_plugin(delta.messages)
            .ok_or("Parsed session record row is invalid")?;
        let mut starts = Vec::new();
        for record in &window.records {
            starts.resize(record.row + 1, u64::MAX);
            starts[record.row] = record.offset - window.meta.from_offset;
        }
        starts.push(window.meta.new_offset - window.meta.from_offset);
        let indexes: Vec<_> = indexes
            .into_iter()
            .map(|index| {
                index.filter(|index| starts.get(*index).is_some_and(|offset| *offset != u64::MAX))
            })
            .collect();
        crate::jsonl_window::anchor_rows_from_starts(
            &mut messages,
            &indexes,
            &starts,
            window.meta.from_offset,
        );
        Ok(SessionDelta {
            messages,
            new_offset: window.meta.new_offset,
        })
    }
}

fn chunk_value(bytes: Vec<u8>) -> serde_json::Value {
    let chunk = String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    serde_json::Value::String(chunk)
}

#[cfg(test)]
mod tests;
