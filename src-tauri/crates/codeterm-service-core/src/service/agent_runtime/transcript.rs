use super::{
    engine::Session,
    events,
    model::{ItemCache, RuntimeFailure, REREADABLE_CACHE_ROWS},
};
use crate::service::tab_chat::chatbackend_history::{self, ChatBackendHistory};
use codeterm_plugin::agent_runtime::{AgentRuntimeCapability, RuntimeContext};
use codeterm_types::agent::chat_types::ChatMsg;
use parking_lot::Mutex;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

/// Shortest gap between mid-turn appends to the session log, which bounds what a
/// crash can lose without a sync for every streamed row.
const ARCHIVE_TICK_MS: u64 = 5_000;

struct ArchiveWriter {
    path: PathBuf,
    log: ChatBackendHistory,
}

#[derive(Default)]
pub(super) struct TranscriptIo {
    read_failed: AtomicBool,
    write_failed: AtomicBool,
    archive_failed: AtomicBool,
    archive: Mutex<Option<ArchiveWriter>>,
    last_archive_tick_ms: AtomicU64,
}

impl TranscriptIo {
    /// Append the rows changed since the last call. The writer lock spans taking
    /// and writing them, so a later append can never land older row versions.
    fn append_archive(
        &self,
        cache: &Mutex<ItemCache>,
        path: &Path,
        rotated: &Path,
        rotate_at: u64,
        tab: &str,
    ) {
        let mut writer = self.archive.lock();
        let rows = cache.lock().take_archive_rows();
        if rows.is_empty() {
            return;
        }
        let bounded: Vec<ChatMsg> = rows.iter().cloned().map(|row| bounded_row(row).0).collect();
        let result = rotate_archive(&mut writer, path, rotated, rotate_at).and_then(|()| {
            let current = writer.get_or_insert_with(|| ArchiveWriter {
                path: path.to_path_buf(),
                log: ChatBackendHistory::open_at(path.to_path_buf()),
            });
            current.log.try_upsert(&bounded).map(|_| ())
        });
        if let Err(error) = result {
            cache.lock().mark_archive_rows(&rows);
            warn_once(&self.archive_failed, tab, "archive", error);
        }
    }

    fn read_rows(&self, path: &Path, tab: &str) -> Vec<ChatMsg> {
        match chatbackend_history::try_read_messages(path) {
            Ok(rows) => rows,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                warn_once(&self.read_failed, tab, "restore", error);
                Vec::new()
            }
        }
    }

    fn write_rows(&self, writer: &mut ChatBackendHistory, rows: Vec<ChatMsg>, tab: &str) {
        if let Err(error) = writer.snapshot(rows) {
            warn_once(&self.write_failed, tab, "persist", error);
        }
    }
}

fn rotate_archive(
    writer: &mut Option<ArchiveWriter>,
    path: &Path,
    rotated: &Path,
    rotate_at: u64,
) -> std::io::Result<()> {
    if writer.as_ref().is_some_and(|current| current.path != path) {
        *writer = None;
    }
    let full = match std::fs::metadata(path) {
        Ok(metadata) => metadata.len() >= rotate_at,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    if full {
        *writer = None;
        std::fs::rename(path, rotated)?;
    }
    Ok(())
}

fn warn_once(flag: &AtomicBool, tab: &str, operation: &str, error: impl std::fmt::Display) {
    if !flag.swap(true, Ordering::Relaxed) {
        log::warn!("[runtime-transcript] tab={tab} {operation} failed; continuing: {error}");
    }
}

fn same_binding(current: &RuntimeContext, expected: &RuntimeContext) -> bool {
    current.epoch == expected.epoch
        && current.thread_id == expected.thread_id
        && current.binding_nonce == expected.binding_nonce
        && current.cwd == expected.cwd
}

fn enabled(capability: &AgentRuntimeCapability) -> bool {
    !capability.stored_history
        && !capability
            .request_methods
            .get("readHistory")
            .is_some_and(|methods| !methods.is_empty())
}

fn selected(session: &Session) -> bool {
    session
        .adapter
        .lock()
        .as_ref()
        .and_then(|adapter| adapter.agent_runtime())
        .is_some_and(|capability| enabled(&capability))
}

pub(crate) fn persisted(
    provider: &str,
    thread: &str,
    capability: &AgentRuntimeCapability,
) -> std::io::Result<Option<Vec<ChatMsg>>> {
    if !enabled(capability) {
        return Ok(None);
    }
    match chatbackend_history::read_tail(&chatbackend_history::runtime_path(provider, thread)) {
        Ok(window) => Ok(Some(window.keep_last(REREADABLE_CACHE_ROWS).messages)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// The rows this host retained for one thread: its restart snapshot, else the
/// session log of a session that never settled a turn. `None` when neither exists.
pub(crate) fn retained(
    provider: &str,
    thread: &str,
    capability: &AgentRuntimeCapability,
) -> std::io::Result<Option<Vec<ChatMsg>>> {
    if !enabled(capability) {
        return Ok(None);
    }
    if let Some(rows) = persisted(provider, thread, capability)? {
        return Ok(Some(rows));
    }
    Ok(closed_rows(provider, thread)?
        .map(|mut rows| rows.split_off(rows.len().saturating_sub(REREADABLE_CACHE_ROWS))))
}

/// A thread's kept rows when this host is the record of its history; an absent
/// record is an empty conversation. `None` when the provider holds the history.
pub(super) fn kept_rows(
    provider: &str,
    thread: &str,
    capability: &AgentRuntimeCapability,
) -> std::io::Result<Option<Vec<ChatMsg>>> {
    if !enabled(capability) {
        return Ok(None);
    }
    retained(provider, thread, capability).map(|rows| Some(rows.unwrap_or_default()))
}

/// The host's own durable transcript for one thread, when the runtime keeps it.
/// `None` means the record is absent from this host's authority or unreadable.
pub(super) fn durable_rows(
    provider: &str,
    thread: &str,
    capability: &AgentRuntimeCapability,
) -> Option<Vec<ChatMsg>> {
    if !enabled(capability) {
        return None;
    }
    match chatbackend_history::read_tail(&chatbackend_history::runtime_path(provider, thread)) {
        Ok(window) => Some(window.messages),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Vec::new()),
        Err(_) => None,
    }
}

/// Every retained row of one host-kept session: the rotated log, the restart
/// snapshot and the current log, newest record winning. `None` when none exists.
pub(crate) fn closed_rows(provider: &str, thread: &str) -> std::io::Result<Option<Vec<ChatMsg>>> {
    merged_logs(&[
        chatbackend_history::archive_rotated_path(provider, thread),
        chatbackend_history::runtime_path(provider, thread),
        chatbackend_history::archive_path(provider, thread),
    ])
}

fn merged_logs(paths: &[PathBuf]) -> std::io::Result<Option<Vec<ChatMsg>>> {
    let mut found = false;
    let mut rows = std::collections::HashMap::new();
    for path in paths {
        match chatbackend_history::read_log(path) {
            Ok(messages) => {
                found = true;
                for row in messages {
                    rows.insert(row.id.clone(), row);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if !found {
        return Ok(None);
    }
    let mut rows: Vec<ChatMsg> = rows.into_values().collect();
    rows.sort_by(|left, right| {
        left.seq
            .cmp(&right.seq)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(Some(rows))
}

fn archive_now(worker: &Session) {
    let context = worker.context.lock().clone();
    let Some(thread) = context.thread_id.as_deref() else {
        return;
    };
    worker.transcript_io.append_archive(
        &worker.cache,
        &chatbackend_history::archive_path(&worker.provider, thread),
        &chatbackend_history::archive_rotated_path(&worker.provider, thread),
        chatbackend_history::ARCHIVE_ROTATE_BYTES,
        &context.tab_id,
    );
}

/// Flush the session log as the runtime stops. The restart snapshot stays as the
/// last settled turn left it, so restore never sees a half-finished one.
pub(crate) async fn archive(session: &Arc<Session>) {
    if !selected(session) {
        return;
    }
    let worker = session.clone();
    if let Err(error) = tokio::task::spawn_blocking(move || archive_now(&worker)).await {
        let tab = session.context.lock().tab_id.clone();
        warn_once(
            &session.transcript_io.archive_failed,
            &tab,
            "archive",
            error,
        );
    }
}

fn archive_tick_due(last: &AtomicU64, now_ms: u64) -> bool {
    let previous = last.load(Ordering::Relaxed);
    now_ms.saturating_sub(previous) >= ARCHIVE_TICK_MS
        && last
            .compare_exchange(previous, now_ms, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}

/// Mid-turn append, off the event loop and rate-limited before any other work.
pub(crate) fn archive_tick(session: &Arc<Session>) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if !archive_tick_due(&session.transcript_io.last_archive_tick_ms, now_ms) || !selected(session)
    {
        return;
    }
    let worker = session.clone();
    drop(tokio::task::spawn_blocking(move || archive_now(&worker)));
}

pub(crate) async fn restore(session: &Arc<Session>) -> Result<(), RuntimeFailure> {
    if !selected(session) {
        return Ok(());
    }
    session.cache.lock().track_archive();
    let worker = session.clone();
    let result = tokio::task::spawn_blocking(move || {
        let context = worker.context.lock().clone();
        let Some(thread) = context.thread_id.as_deref() else {
            return Ok(());
        };
        let path = chatbackend_history::runtime_path(&worker.provider, thread);
        let rows = worker.transcript_io.read_rows(&path, &context.tab_id);
        let current = {
            let current = worker.context.lock();
            same_binding(&current, &context)
        };
        if !current || !worker.maps.lock().prepare_restore(&context) {
            return Err(RuntimeFailure::Superseded);
        }
        let ordinal = apply_restored_rows(&worker.cache, &worker.maps, &context, rows, || {
            let current = worker.context.lock();
            same_binding(&current, &context)
        })?;
        let mut current = worker.context.lock();
        if !same_binding(&current, &context) {
            return Err(RuntimeFailure::Superseded);
        }
        current.restored_row_ordinal =
            Some(ordinal.max(current.restored_row_ordinal.unwrap_or_default()));
        Ok(())
    })
    .await;
    match result {
        Ok(result) => result,
        Err(error) => {
            let tab = session.context.lock().tab_id.clone();
            warn_once(&session.transcript_io.read_failed, &tab, "restore", error);
            Ok(())
        }
    }
}

pub(super) fn apply_restored_rows(
    cache: &Mutex<ItemCache>,
    maps: &Mutex<events::RuntimeMaps>,
    context: &RuntimeContext,
    rows: Vec<ChatMsg>,
    is_current: impl Fn() -> bool,
) -> Result<u64, RuntimeFailure> {
    let thread = context
        .thread_id
        .as_deref()
        .ok_or(RuntimeFailure::Superseded)?;
    if !is_current() {
        return Err(RuntimeFailure::Superseded);
    }
    let ordinal = {
        let mut cache = cache.lock();
        cache.restore_transcript(rows.clone());
        cache.next_ordinal()
    };
    for row in rows {
        if !is_current() {
            return Err(RuntimeFailure::Superseded);
        }
        let mut maps = maps.lock();
        if !events::reconcile_history_current(&mut maps, context, thread, row) {
            return Err(RuntimeFailure::Superseded);
        }
    }
    Ok(ordinal)
}

pub(crate) async fn persist(session: &Arc<Session>) {
    if !selected(session) {
        return;
    }
    let worker = session.clone();
    let result = tokio::task::spawn_blocking(move || {
        let context = worker.context.lock().clone();
        let Some(thread) = context.thread_id.as_deref() else {
            return;
        };
        let rows = bounded_rows(worker.cache.lock().tail(REREADABLE_CACHE_ROWS));
        let current = {
            let current = worker.context.lock();
            same_binding(&current, &context)
        };
        if !current {
            return;
        }
        worker.transcript_io.write_rows(
            &mut ChatBackendHistory::runtime_shared(&worker.provider, thread).lock(),
            rows,
            &context.tab_id,
        );
        archive_now(&worker);
    })
    .await;
    if let Err(error) = result {
        let tab = session.context.lock().tab_id.clone();
        warn_once(&session.transcript_io.write_failed, &tab, "persist", error);
    }
}

fn bounded_rows(rows: Vec<ChatMsg>) -> Vec<ChatMsg> {
    let mut retained = Vec::new();
    let mut bytes = 0;
    for row in rows.into_iter().rev() {
        let (row, size) = bounded_row(row);
        if bytes + size > chatbackend_history::TAIL_BYTES as usize {
            break;
        }
        bytes += size;
        retained.push(row);
    }
    retained.reverse();
    retained
}

fn bounded_row(mut row: ChatMsg) -> (ChatMsg, usize) {
    let mut size =
        serde_json::to_vec(&row).map_or(usize::MAX, |bytes| bytes.len().saturating_add(1));
    if size > chatbackend_history::TAIL_BYTES as usize {
        row = ChatMsg {
            id: row.id,
            seq: row.seq,
            kind: row.kind,
            content: "[Runtime transcript row truncated: history budget exceeded]".into(),
            ..Default::default()
        };
        size = serde_json::to_vec(&row).map_or(usize::MAX, |bytes| bytes.len().saturating_add(1));
    }
    (row, size)
}

#[cfg(test)]
mod tests;
