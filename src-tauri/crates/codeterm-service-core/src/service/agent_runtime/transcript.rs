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
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Default)]
pub(super) struct TranscriptIo {
    read_failed: AtomicBool,
    write_failed: AtomicBool,
}

impl TranscriptIo {
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
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Some(Vec::new())),
        Err(error) => Err(error),
    }
}

pub(crate) async fn restore(session: &Arc<Session>) -> Result<(), RuntimeFailure> {
    if !selected(session) {
        return Ok(());
    }
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
    for mut row in rows.into_iter().rev() {
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
            size =
                serde_json::to_vec(&row).map_or(usize::MAX, |bytes| bytes.len().saturating_add(1));
        }
        if bytes + size > chatbackend_history::TAIL_BYTES as usize {
            break;
        }
        bytes += size;
        retained.push(row);
    }
    retained.reverse();
    retained
}

#[cfg(test)]
mod tests;
