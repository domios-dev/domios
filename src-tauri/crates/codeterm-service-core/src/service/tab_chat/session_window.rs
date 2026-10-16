//! Bounded initial snapshots over a session JSONL. Chat init must never read a
//! whole session file: production rollouts reach hundreds of MB and the read
//! lands on the daemon's async runtime.

use std::path::Path;
use std::sync::Arc;

use codeterm_plugin::jsonl_window::page_before;

use super::results::{ChatStartKind, KnownChatWindow};
use super::resume_cursor::{plan_resume, ChatEpoch, ResumeCursor, ResumePlan};
use crate::agent::chat_types::{ChatMsg, RichMessage};
use crate::agent::registry::AgentRegistry;

pub fn attach_kind(
    known: &KnownChatWindow,
    epoch: &ChatEpoch,
    incarnation: &str,
    total: u64,
) -> ChatStartKind {
    if known.source_session_id != epoch.source_session_id
        || known.session_binding_nonce != epoch.session_binding_nonce
        || known.end_offset > known.file_length
        || known.file_length > total
        || total.saturating_sub(known.end_offset) >= SNAPSHOT_WINDOW_BYTES
    {
        return ChatStartKind::Snapshot;
    }
    let Ok(cursor) = ResumeCursor::decode(&known.cursor) else {
        return ChatStartKind::Snapshot;
    };
    if !matches!(&cursor, ResumeCursor::Bytes { offset, .. } if *offset == known.end_offset)
        || !matches!(
            plan_resume(
                &cursor,
                epoch,
                incarnation,
                total,
                true,
                SNAPSHOT_WINDOW_BYTES
            ),
            ResumePlan::Empty | ResumePlan::Read { .. }
        )
    {
        return ChatStartKind::Snapshot;
    }
    if total == known.file_length {
        ChatStartKind::Unchanged
    } else {
        ChatStartKind::Delta
    }
}

pub fn resume_window(
    registry: &Arc<AgentRegistry>,
    provider_id: &str,
    source: &super::registry::JsonlSource,
    path: &Path,
    epoch: &ChatEpoch,
    known: &KnownChatWindow,
) -> Option<(ChatStartKind, SessionSnapshot)> {
    let total = std::fs::metadata(path).ok()?.len();
    let incarnation =
        super::resume::capture_incarnation(source, epoch, path, known.end_offset, None)?;
    let kind = attach_kind(known, epoch, &incarnation.id, total);
    if kind == ChatStartKind::Snapshot || !super::resume::boundary(path, known.end_offset).ok()? {
        return None;
    }
    if kind == ChatStartKind::Unchanged {
        log::debug!(
            "[chat_start.unchanged] session={} end={} total_bytes={}",
            epoch.source_session_id,
            known.end_offset,
            total
        );
        return Some((
            kind,
            SessionSnapshot {
                total,
                start_offset: known.end_offset,
                end_offset: known.end_offset,
                ..Default::default()
            },
        ));
    }
    let (rich, end_offset) = parse_window(registry, provider_id, path, known.end_offset, total);
    let current =
        super::resume::capture_incarnation(source, epoch, path, end_offset, Some(&incarnation))?;
    if current != incarnation {
        return None;
    }
    Some((
        kind,
        SessionSnapshot {
            row_record_starts: Vec::new(),
            context_usage: rich.iter().rev().find_map(|row| row.context_usage.clone()),
            messages: super::rich_rows_to_chat_msgs(&rich, known.end_offset),
            runtime_observations: rich,
            total,
            start_offset: known.end_offset,
            end_offset,
            bytes_read: end_offset.saturating_sub(known.end_offset),
        },
    ))
}

/// Above this the snapshot switches from "whole transcript" to a tail window.
pub const SNAPSHOT_CAP_BYTES: u64 = 1024 * 1024;
/// Size of that tail window, anchored forward to a record boundary.
pub const SNAPSHOT_WINDOW_BYTES: u64 = SNAPSHOT_CAP_BYTES;

#[derive(Clone, Default)]
pub struct SessionSnapshot {
    pub row_record_starts: Vec<u64>,
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
        row_record_starts: messages
            .iter()
            .map(|message| {
                usize::try_from(message.seq)
                    .ok()
                    .and_then(|index| rich.get(index))
                    .and_then(|row| row.record_start)
                    .unwrap_or(start_offset)
            })
            .collect(),
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
