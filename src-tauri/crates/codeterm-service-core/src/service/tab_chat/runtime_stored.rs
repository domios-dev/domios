use super::results::{ChatBackfillResult, ChatOffsetKind, ChatStartResult};
use crate::{agent::chat_types::ChatMsg, service::ServiceContext};
use std::{collections::HashSet, path::Path, sync::LazyLock};

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
                        &svc.app_handle,
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

fn read_window(path: &Path, before: Option<u64>) -> Result<(String, u64, u64), String> {
    let total = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    let end = before.unwrap_or(total).min(total);
    let start = codeterm_plugin::jsonl_window::tail_window_start(path, end, WINDOW_BYTES);
    let (mut bytes, start) = codeterm_plugin::jsonl_window::read_range(path, start, end)
        .map_err(|error| error.to_string())?;
    let complete = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |last| last + 1);
    bytes.truncate(complete);
    let records = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    Ok((records, start, start + complete as u64))
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
    let started = std::time::Instant::now();
    let (records, start, end) = read_window(&path, before)?;
    let messages = provider.runtime_stored_history(session, &records)?;
    let current = svc.mux.read().get_tab(tab_id).is_some_and(|current| {
        current.agent_type == tab.agent_type
            && current.agent_session_id == tab.agent_session_id
            && current.session_binding_nonce == tab.session_binding_nonce
    });
    if !current {
        return Err("Transcript binding changed during stored read".into());
    }
    log::debug!(
        "[chat-attach] tab={tab_id} phase=stored_snapshot elapsed_ms={} bytes={} rows={}",
        started.elapsed().as_millis(),
        records.len(),
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
    let tab = { svc.mux.read().get_tab(tab_id).cloned() };
    let live = tab.is_some_and(|tab| {
        !tab.frozen && crate::service::agent_runtime::engine::has_live_bound_session(&tab)
    });
    if live {
        return Ok(None);
    }
    let Some(window) = read(svc, tab_id, None)? else {
        return Ok(None);
    };
    if !window.frozen {
        start_runtime(svc, tab_id, window.nonce.as_deref());
    }
    Ok(Some(ChatStartResult {
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
