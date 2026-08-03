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
    /// Exact complete-record boundary the parse ended on — where the tailer
    /// picks up. Below `total` when the file ends in a half-written record, so
    /// that record is consumed once by the tailer instead of being skipped.
    pub end_offset: u64,
    pub bytes_read: u64,
}

impl SessionSnapshot {
    pub fn truncated(&self) -> bool {
        self.start_offset > 0
    }
}

/// Growth that must accumulate before a bounded window re-anchors. Row ids are
/// ordinals inside their parsed window, so an anchor that moved on every attach
/// would re-key the whole overlap on every Terminal -> Chat switch. Quantizing
/// the anchor makes repeated snapshots of a growing session return the exact
/// same window — and therefore the exact same rows and ids — until the file
/// grows past one stride.
pub const SNAPSHOT_ANCHOR_STRIDE_BYTES: u64 = SNAPSHOT_WINDOW_BYTES;

/// Offset the bounded snapshot should begin at, aligned to a record boundary.
/// The window spans `[start, total)` and is therefore between one and two
/// strides wide — still bounded, and stable across reattachment.
pub fn snapshot_start_offset(path: &Path, total: u64) -> u64 {
    if total <= SNAPSHOT_CAP_BYTES {
        return 0;
    }
    let rough = total.saturating_sub(SNAPSHOT_WINDOW_BYTES);
    let anchor = rough - (rough % SNAPSHOT_ANCHOR_STRIDE_BYTES);
    if anchor == 0 {
        return 0;
    }
    tail_window_start(path, anchor.saturating_add(SNAPSHOT_WINDOW_BYTES), SNAPSHOT_WINDOW_BYTES)
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

/// One page of older history ending at `until`. Sized to the provider's own
/// delta chunk so a single capped read stops on the last record boundary at or
/// before `until`: a narrower request would overshoot into already-committed
/// bytes, which byte-range reconciliation cannot absorb.
pub const BACKFILL_PAGE_BYTES: u64 = codeterm_plugin::jsonl_window::DELTA_CHUNK_BYTES;

pub struct SessionPage {
    pub messages: Vec<RichMessage>,
    /// Exact record boundary the read ended on. Above `until` only when a
    /// single record is wider than the page, which the client refuses rather
    /// than adopting a range that misdescribes its rows.
    pub end_offset: u64,
}

/// Parse exactly one [`BACKFILL_PAGE_BYTES`] page ending at `until`. `from`
/// may land mid-record; the provider drops that partial head, so the caller
/// reports the first complete-record boundary as the page start.
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
    let messages = rich
        .iter()
        .enumerate()
        .flat_map(|(i, r)| super::rich_to_chat_msgs_at(r, super::RowOrigin::at(start_offset, i)))
        .collect();
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
