use super::results::{ChatBackfillResult, ChatOffsetKind, ChatStartResult};
use crate::{agent::chat_types::ChatMsg, service::ServiceContext};
use std::{collections::HashSet, path::Path, sync::LazyLock};

mod projection;

const WINDOW_BYTES: u64 = super::session_window::SNAPSHOT_WINDOW_BYTES;
static STARTING: LazyLock<parking_lot::Mutex<HashSet<(String, String)>>> =
    LazyLock::new(Default::default);

struct Starting {
    key: (String, String),
}

impl Drop for Starting {
    fn drop(&mut self) {
        STARTING.lock().remove(&self.key);
    }
}

fn start_runtime(svc: &ServiceContext, tab: &str, nonce: Option<&str>) {
    let Some(nonce) = nonce else { return };
    let key = (tab.to_owned(), nonce.to_owned());
    if !STARTING.lock().insert(key.clone()) {
        return;
    }
    let starting = Starting { key };
    let svc = svc.clone();
    crate::async_runtime::spawn_blocking(move || {
        let current = svc.mux.read().get_tab(&starting.key.0).is_some_and(|tab| {
            !tab.frozen && tab.session_binding_nonce.as_deref() == Some(&starting.key.1)
        });
        if current {
            match crate::async_runtime::block_on(crate::service::agent_runtime::engine::ensure(
                &svc,
                &starting.key.0,
            )) {
                Ok(session) => {
                    if crate::service::agent_runtime::engine::validate_binding(&svc, &session)
                        .is_err()
                    {
                        return;
                    }
                    let context = session.context.lock().clone();
                    let rows = session.cache.lock().tail(200);
                    super::structured_sink::emit_chat_msgs(
                        svc.ui.as_ref(),
                        &starting.key.0,
                        &rows,
                        None,
                        context.thread_id.as_deref(),
                        Some(&context.binding_nonce),
                    );
                }
                Err(reason) => log::debug!(
                    "[chat-attach] tab={} background runtime failed: {reason}",
                    starting.key.0
                ),
            }
        }
    });
}

struct Window {
    messages: Vec<ChatMsg>,
    session: String,
    nonce: Option<String>,
    start: u64,
    end: u64,
    frozen: bool,
}

enum StoredInput {
    Text(String, u64, u64),
    Records(codeterm_plugin::jsonl_window::records::RecordWindow),
}

fn read_window(path: &Path, before: Option<u64>) -> Result<(String, u64, u64), String> {
    let total = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    let end = before.unwrap_or(total).min(total);
    let page = codeterm_plugin::jsonl_window::page_before(path, end, WINDOW_BYTES)
        .map_err(|error| error.to_string())?;
    if page.end > 0 && page.start >= page.end {
        return Err("Stored history record exceeds backward scan limit".into());
    }
    let (bytes, start) = codeterm_plugin::jsonl_window::read_range(path, page.start, page.end)
        .map_err(|error| error.to_string())?;
    let records = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    let records = if records.len() as u64 > WINDOW_BYTES {
        projection::oversized_record(&records, WINDOW_BYTES as usize)?
    } else {
        records
    };
    Ok((records, start, page.end))
}

fn read(svc: &ServiceContext, tab_id: &str, before: Option<u64>) -> Result<Option<Window>, String> {
    let tab = svc
        .mux
        .read()
        .get_tab(tab_id)
        .cloned()
        .ok_or("Tab not found")?;
    let Some(provider) = tab
        .agent_type
        .as_deref()
        .and_then(|id| svc.agent_registry.provider_arc(id))
    else {
        return Ok(None);
    };
    if !provider
        .agent_runtime()
        .is_some_and(|capability| capability.stored_history)
    {
        return Ok(None);
    }
    let Some(session) = tab
        .agent_session_id
        .as_deref()
        .filter(|session| !session.is_empty())
    else {
        return Ok(None);
    };
    let path = provider
        .session_file_path(tab.cwd.as_deref().unwrap_or_default(), session)
        .ok_or("Provider has no recorded runtime session")?;
    if !path.try_exists().map_err(|error| error.to_string())?
        && crate::service::agent_runtime::engine::has_live_bound_session(&tab)
    {
        return Ok(None);
    }
    let started = std::time::Instant::now();
    let input = if provider.record_source("storedHistory").is_some() {
        StoredInput::Records(super::jsonl_kernel::stored_window(
            provider.as_ref(),
            session,
            &path,
            before,
            WINDOW_BYTES,
        )?)
    } else {
        let (records, start, end) = read_window(&path, before)?;
        StoredInput::Text(records, start, end)
    };
    let read_us = started.elapsed().as_micros();
    let projecting = std::time::Instant::now();
    let (messages, start, end) = match input {
        StoredInput::Text(records, start, end) => (
            provider.runtime_stored_history(session, &records)?,
            start,
            end,
        ),
        StoredInput::Records(window) => (
            provider.runtime_stored_records(session, &window)?,
            window.meta.from_offset,
            window.meta.new_offset,
        ),
    };
    let projection_us = projecting.elapsed().as_micros();
    let current = svc.mux.read().get_tab(tab_id).is_some_and(|current| {
        current.agent_type == tab.agent_type
            && current.agent_session_id == tab.agent_session_id
            && current.session_binding_nonce == tab.session_binding_nonce
    });
    if !current {
        return Err("Transcript binding changed during stored read".into());
    }
    log::debug!(
        "[chat-attach] tab={tab_id} phase=stored_snapshot elapsed_ms={} read_us={read_us} projection_us={projection_us} bytes={} rows={}",
        started.elapsed().as_millis(),
        end.saturating_sub(start),
        messages.len()
    );
    Ok(Some(Window {
        messages,
        session: session.to_owned(),
        nonce: tab.session_binding_nonce,
        start,
        end,
        frozen: tab.frozen,
    }))
}

pub fn start(
    svc: &ServiceContext,
    tab_id: &str,
    subscription: &str,
) -> Result<Option<ChatStartResult>, String> {
    let Some(window) = read(svc, tab_id, None)? else {
        return Ok(None);
    };
    super::structured_sink::seed_snapshot(
        tab_id,
        Some(&window.session),
        window.nonce.as_deref(),
        &window.messages,
    );
    if !window.frozen {
        start_runtime(svc, tab_id, window.nonce.as_deref());
    }
    Ok(Some(ChatStartResult {
        kind: Default::default(),
        file_length: None,
        text: String::new(),
        offset_kind: ChatOffsetKind::Bytes,
        start_offset: window.start,
        offset: window.end,
        pending: false,
        messages: Some(window.messages),
        cursor: None,
        source_session_id: Some(window.session),
        session_binding_nonce: window.nonce,
        subscription_id: Some(subscription.to_owned()),
        source_missing: false,
    }))
}

pub fn backfill(
    svc: &ServiceContext,
    tab_id: &str,
    before: u64,
) -> Result<Option<ChatBackfillResult>, String> {
    Ok(
        read(svc, tab_id, Some(before))?.map(|window| ChatBackfillResult {
            text: String::new(),
            offset_kind: ChatOffsetKind::Bytes,
            start_offset: window.start,
            end_offset: window.end,
            messages: window.messages,
            source_session_id: Some(window.session),
            session_binding_nonce: window.nonce,
        }),
    )
}

pub(super) fn messages(
    svc: &ServiceContext,
    tab_id: &str,
    since: usize,
) -> Result<Option<(Vec<ChatMsg>, usize)>, String> {
    Ok(read(svc, tab_id, None)?.map(|window| {
        let total = window.messages.len();
        (window.messages.into_iter().skip(since).collect(), total)
    }))
}

#[cfg(test)]
mod tests;
