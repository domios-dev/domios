use super::{
    engine::Session,
    events,
    model::{RuntimeFailure, REREADABLE_CACHE_ROWS},
};
use crate::service::tab_chat::chatbackend_history::{self, ChatBackendHistory};
use codeterm_plugin::agent_runtime::AgentRuntimeCapability;
use codeterm_types::agent::chat_types::ChatMsg;
use std::sync::Arc;

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

pub(super) async fn restore(session: &Arc<Session>) -> Result<(), RuntimeFailure> {
    if !selected(session) {
        return Ok(());
    }
    let session = session.clone();
    tokio::task::spawn_blocking(move || {
        let context = session.context.lock().clone();
        let Some(thread) = context.thread_id.as_deref() else {
            return Ok(());
        };
        let path = chatbackend_history::runtime_path(&session.provider, thread);
        let rows = match chatbackend_history::try_read_messages(&path) {
            Ok(rows) => rows,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(RuntimeFailure::HookFailed(error.to_string())),
        };
        let mut current = session.context.lock();
        if current.epoch != context.epoch
            || current.thread_id != context.thread_id
            || current.binding_nonce != context.binding_nonce
            || current.cwd != context.cwd
        {
            return Err(RuntimeFailure::Superseded);
        }
        let ordinal = {
            let mut cache = session.cache.lock();
            cache.restore_transcript(rows.clone());
            cache.next_ordinal()
        };
        for row in rows {
            events::reconcile_history(&mut session.maps.lock(), context.epoch, thread, row);
        }
        current.restored_row_ordinal =
            Some(ordinal.max(current.restored_row_ordinal.unwrap_or_default()));
        Ok(())
    })
    .await
    .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
}

pub(super) async fn persist(session: &Arc<Session>) -> Result<(), RuntimeFailure> {
    if !selected(session) {
        return Ok(());
    }
    let session = session.clone();
    tokio::task::spawn_blocking(move || {
        let (thread, rows) = {
            let context = session.context.lock();
            let Some(thread) = context.thread_id.as_deref() else {
                return Ok(());
            };
            (
                thread.to_owned(),
                session.cache.lock().tail(REREADABLE_CACHE_ROWS),
            )
        };
        let rows = bounded_rows(rows);
        ChatBackendHistory::runtime_shared(&session.provider, &thread)
            .lock()
            .snapshot(rows)
            .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))
    })
    .await
    .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
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
