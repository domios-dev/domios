//! Bounded initial snapshots over a session JSONL. Chat init must never read a
//! whole session file: production rollouts reach hundreds of MB and the read
//! lands on the daemon's async runtime.

use std::path::Path;
use std::sync::Arc;

use codeterm_plugin::jsonl_window::page_before;

use crate::agent::chat_types::{ChatMsg, RichMessage};
use crate::agent::registry::AgentRegistry;

/// Above this the snapshot switches from "whole transcript" to a tail window.
pub const SNAPSHOT_CAP_BYTES: u64 = 1024 * 1024;
/// Size of that tail window, anchored forward to a record boundary.
pub const SNAPSHOT_WINDOW_BYTES: u64 = SNAPSHOT_CAP_BYTES;

#[derive(Clone, Default)]
pub struct SessionSnapshot {
    pub runtime_observations: Vec<RichMessage>,
    pub context_usage: Option<codeterm_types::agent::context::TurnContext>,
    pub messages: Vec<ChatMsg>,
    /// File size at snapshot time.
    pub total: u64,
    /// Where the snapshot started reading; `0` means the full transcript.
    pub start_offset: u64,
    /// Record boundary the parse ended on; the tailer resumes here so a
    /// half-written trailing record is consumed once instead of skipped.
    pub end_offset: u64,
    pub bytes_read: u64,
}

impl SessionSnapshot {
    pub fn truncated(&self) -> bool {
        self.start_offset > 0
    }
}

pub fn snapshot_start_offset(path: &Path, total: u64) -> u64 {
    page_before(path, total, SNAPSHOT_WINDOW_BYTES).map_or(total, |page| page.start)
}

/// Parse complete records using the remaining window as the host read budget.
pub fn parse_window(
    registry: &Arc<AgentRegistry>,
    provider_id: &str,
    path: &Path,
    from: u64,
    until: u64,
) -> (Vec<RichMessage>, u64) {
    let mut offset = from;
    let mut out: Vec<RichMessage> = Vec::new();
    while offset < until {
        let Some(delta) = registry
            .with_provider(provider_id, |p| {
                let mut delta = p.parse_session_delta_bounded(path, offset, until - offset)?;
                for m in &mut delta.messages {
                    super::enrich_interactive(m, p);
                }
                Some(delta)
            })
            .flatten()
        else {
            break;
        };
        if delta.new_offset <= offset {
            break;
        }
        offset = delta.new_offset;
        out.extend(delta.messages);
    }
    (out, offset)
}

/// Soft byte limit for one complete-record history page.
pub const BACKFILL_PAGE_BYTES: u64 = 1024 * 1024;

pub struct SessionPage {
    pub messages: Vec<RichMessage>,
    pub start_offset: u64,
    /// Record boundary the read ended on; above `until` only for a record
    /// wider than the page, which the client then refuses.
    pub end_offset: u64,
}

/// Parse one complete-record page ending at or below `until`.
pub fn parse_page_before(
    registry: &Arc<AgentRegistry>,
    provider_id: &str,
    path: &Path,
    until: u64,
) -> Result<SessionPage, String> {
    let page = page_before(path, until, BACKFILL_PAGE_BYTES).map_err(|error| error.to_string())?;
    let (messages, end_offset) = parse_window(registry, provider_id, path, page.start, page.end);
    Ok(SessionPage {
        messages,
        start_offset: page.start,
        end_offset,
    })
}

/// Bounded snapshot of a JSONL session: the whole transcript for ordinary
/// files, the trailing [`SNAPSHOT_WINDOW_BYTES`] for oversized ones.
pub fn snapshot(registry: &Arc<AgentRegistry>, provider_id: &str, path: &Path) -> SessionSnapshot {
    let total = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let page = page_before(path, total, SNAPSHOT_WINDOW_BYTES).unwrap_or(
        codeterm_plugin::jsonl_window::RecordPage {
            start: total,
            end: total,
        },
    );
    let (rich, end) = parse_window(registry, provider_id, path, page.start, page.end);
    let (start_offset, messages) = super::snapshot_rows::retain_tail(&rich, page.start);
    SessionSnapshot {
        runtime_observations: rich
            .iter()
            .map(|row| RichMessage {
                provider_session_id: row.provider_session_id.clone(),
                provider_observations: row.provider_observations.clone(),
                background_work: row.background_work.clone(),
                blocks: row
                    .blocks
                    .iter()
                    .filter(|block| block.data.get("tool_background_task_id").is_some())
                    .cloned()
                    .collect(),
                ..Default::default()
            })
            .collect(),
        context_usage: rich
            .iter()
            .rev()
            .find_map(|message| message.context_usage.clone()),
        messages,
        total,
        start_offset,
        end_offset: end,
        bytes_read: end.saturating_sub(page.start),
    }
}

/// Emitted once per oversized-session chat init so a slow chat has a marker in
/// the log even when the bounded read kept it under the slow-init threshold.
pub fn log_bounded_snapshot(scope: &str, provider_id: &str, snap: &SessionSnapshot) {
    if !snap.truncated() {
        return;
    }
    log::warn!(
        "[chat_start.bounded] scope={} provider={} total_bytes={} window_start={} window_end={} bytes_read={} messages={}",
        scope,
        provider_id,
        snap.total,
        snap.start_offset,
        snap.end_offset,
        snap.bytes_read,
        snap.messages.len()
    );
}

#[cfg(test)]
mod tests;
