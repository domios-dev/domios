//! Bounded initial snapshots over a session JSONL. Chat init must never read a
//! whole session file: production rollouts reach hundreds of MB and the read
//! lands on the daemon's async runtime.

use std::path::Path;
use std::sync::Arc;

use codeterm_plugin::jsonl_window::tail_window_start;

use crate::agent::chat_types::{ChatMsg, RichMessage};
use crate::agent::registry::AgentRegistry;

/// Above this the snapshot switches from "whole transcript" to a tail window.
pub const SNAPSHOT_CAP_BYTES: u64 = 10 * 1024 * 1024;
/// Size of that tail window, anchored forward to a record boundary.
pub const SNAPSHOT_WINDOW_BYTES: u64 = 4 * 1024 * 1024;

pub struct SessionSnapshot {
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

/// Growth needed before a bounded window re-anchors. A moving anchor would
/// re-key the whole overlap on every reattachment.
pub const SNAPSHOT_ANCHOR_STRIDE_BYTES: u64 = SNAPSHOT_WINDOW_BYTES;

/// Record-aligned start of the bounded snapshot; the window spans
/// `[start, total)` and is one to two strides wide.
pub fn snapshot_start_offset(path: &Path, total: u64) -> u64 {
    if total <= SNAPSHOT_CAP_BYTES {
        return 0;
    }
    let rough = total.saturating_sub(SNAPSHOT_WINDOW_BYTES);
    let anchor = rough - (rough % SNAPSHOT_ANCHOR_STRIDE_BYTES);
    if anchor == 0 {
        return 0;
    }
    tail_window_start(
        path,
        anchor.saturating_add(SNAPSHOT_WINDOW_BYTES),
        SNAPSHOT_WINDOW_BYTES,
    )
}

/// Parse `[from, until)` by draining the provider's capped delta reads. Each
/// call consumes at most [`DELTA_CHUNK_BYTES`], so a window wider than the cap
/// needs several passes; a non-advancing offset ends the drain.
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
        let Some(delta) = registry.with_provider(provider_id, |p| {
            let mut delta = p.parse_session_delta(path, offset);
            for m in &mut delta.messages {
                super::enrich_interactive(m, p);
            }
            delta
        }) else {
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

/// One page of older history, sized to the provider delta chunk because a
/// narrower request overshoots into already-committed bytes.
pub const BACKFILL_PAGE_BYTES: u64 = codeterm_plugin::jsonl_window::DELTA_CHUNK_BYTES;

pub struct SessionPage {
    pub messages: Vec<RichMessage>,
    /// Record boundary the read ended on; above `until` only for a record
    /// wider than the page, which the client then refuses.
    pub end_offset: u64,
}

/// Parse one [`BACKFILL_PAGE_BYTES`] page ending at `until`. The provider drops
/// the partial head, so the caller reports the first complete record as start.
pub fn parse_page_before(
    registry: &Arc<AgentRegistry>,
    provider_id: &str,
    path: &Path,
    until: u64,
) -> SessionPage {
    let from = until.saturating_sub(BACKFILL_PAGE_BYTES);
    let Some(delta) = registry.with_provider(provider_id, |p| {
        let mut delta = p.parse_session_delta(path, from);
        for m in &mut delta.messages {
            super::enrich_interactive(m, p);
        }
        delta
    }) else {
        return SessionPage {
            messages: Vec::new(),
            end_offset: until,
        };
    };
    SessionPage {
        messages: delta.messages,
        end_offset: delta.new_offset,
    }
}

/// Bounded snapshot of a JSONL session: the whole transcript for ordinary
/// files, the trailing [`SNAPSHOT_WINDOW_BYTES`] for oversized ones.
pub fn snapshot(registry: &Arc<AgentRegistry>, provider_id: &str, path: &Path) -> SessionSnapshot {
    let total = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let start_offset = snapshot_start_offset(path, total);
    let (rich, end) = parse_window(registry, provider_id, path, start_offset, total);
    let messages = super::rich_rows_to_chat_msgs(&rich, start_offset);
    SessionSnapshot {
        messages,
        total,
        start_offset,
        end_offset: end,
        bytes_read: end.saturating_sub(start_offset),
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
