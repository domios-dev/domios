//! Reads the transcript of an agent tab that has closed, from the record of which
//! session held it. Authority belongs to the caller; nothing here wakes a runtime.

use super::{chatbackend_history, runtime_stored::StoredHistoryFailure, ChatMsg};
use crate::multiplexer::state::ClosedTranscript;
use crate::service::{agent_runtime::engine, ServiceContext};

pub struct ClosedTranscriptSnapshot {
    pub messages: Vec<ChatMsg>,
    pub count: usize,
    /// The source held older records than this read could return.
    pub truncated: bool,
}

pub fn snapshot(
    svc: &ServiceContext,
    record: &ClosedTranscript,
    since: usize,
) -> Result<ClosedTranscriptSnapshot, String> {
    let (current, start) = current_rows(svc, record)?;
    // A tab that returned to an earlier session must not read that session twice.
    let archives: Vec<_> = record
        .archives
        .iter()
        .filter(|archive| {
            archive.provider != record.provider || archive.session_id != record.session_id
        })
        .cloned()
        .collect();
    let messages = super::session_segments::project_with_archived_provider_sessions(
        svc,
        &archives,
        &record.session_id,
        current,
    );
    let count = messages.len();
    Ok(ClosedTranscriptSnapshot {
        messages: messages
            .into_iter()
            .skip(since)
            .map(ChatMsg::with_segments)
            .collect(),
        count,
        truncated: start > 0,
    })
}

fn current_rows(
    svc: &ServiceContext,
    record: &ClosedTranscript,
) -> Result<(Vec<ChatMsg>, u64), String> {
    if record.session_id.is_empty() {
        return Ok((Vec::new(), 0));
    }
    if let Some(rows) = engine::closed_transcript_rows(&record.provider, &record.session_id)
        .map_err(|error| {
            StoredHistoryFailure::Unavailable.error(format!("Closed transcript: {error}"))
        })?
    {
        return Ok((rows, 0));
    }
    if record.chat_backend {
        let path = chatbackend_history::session_path(&record.provider, &record.session_id);
        match chatbackend_history::read_tail(&path) {
            Ok(window) => return Ok((window.messages, window.start_offset)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(
                    StoredHistoryFailure::Unavailable.error(format!("Closed transcript: {error}"))
                )
            }
        }
    }
    super::session_chat_window(svc, &record.provider, &record.cwd, &record.session_id).map_err(
        |error| {
            if StoredHistoryFailure::from_error(&error).is_some() {
                error
            } else {
                StoredHistoryFailure::Missing.error(error)
            }
        },
    )
}

/// Retention dropped these records: remove their session logs unless a live tab
/// resumed the session. The restart snapshot stays, since reopen history outlives
/// this index and a reopened tab restores from it.
pub fn forget_evicted(svc: &ServiceContext) {
    let (evicted, live) = {
        let mux = svc.mux.read();
        let evicted = mux.take_evicted_closed_transcripts();
        if evicted.is_empty() {
            return;
        }
        let live: std::collections::HashSet<String> = mux
            .all_tabs()
            .filter_map(|tab| tab.agent_session_id.clone())
            .collect();
        (evicted, live)
    };
    for record in evicted {
        if live.contains(&record.session_id) {
            continue;
        }
        for path in [
            chatbackend_history::archive_path(&record.provider, &record.session_id),
            chatbackend_history::archive_rotated_path(&record.provider, &record.session_id),
        ] {
            if let Err(error) = std::fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("[closed-history] session log not removed: {error}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
