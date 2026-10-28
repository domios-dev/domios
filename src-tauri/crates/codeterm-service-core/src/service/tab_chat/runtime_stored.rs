use super::results::{ChatBackfillResult, ChatOffsetKind, ChatStartResult};
use crate::{
    agent::{chat_types::ChatMsg, provider::AgentProvider},
    service::ServiceContext,
};
use std::{collections::HashSet, path::Path, sync::LazyLock};

mod projection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredHistoryFailure {
    Missing,
    Corrupt,
    Unavailable,
}

impl StoredHistoryFailure {
    fn tag(self) -> &'static str {
        match self {
            Self::Missing => "runtime_history_missing",
            Self::Corrupt => "runtime_history_corrupt",
            Self::Unavailable => "runtime_history_unavailable",
        }
    }

    pub fn from_error(error: &str) -> Option<Self> {
        [Self::Missing, Self::Corrupt, Self::Unavailable].into_iter()
            .find(|kind| error.strip_prefix(kind.tag()).is_some_and(|tail| tail.starts_with(": ")))
    }

    fn projection(error: String) -> String {
        if Self::from_error(&error).is_some() {
            error
        } else {
            Self::Corrupt.error(error)
        }
    }

    fn error(self, detail: impl std::fmt::Display) -> String {
        format!("{}: {detail}", self.tag())
    }
}

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

struct StoredWindow {
    messages: Vec<ChatMsg>,
    start: u64,
    end: u64,
    elapsed_ms: u128,
    read_us: u128,
    projection_us: u128,
}

enum StoredInput {
    Text(String, u64, u64),
    Records(codeterm_plugin::jsonl_window::records::RecordWindow),
}

fn read_window(path: &Path, before: Option<u64>) -> Result<(String, u64, u64), String> {
    let total = std::fs::metadata(path)
        .map_err(|error| StoredHistoryFailure::Unavailable.error(error))?
        .len();
    let end = before.unwrap_or(total).min(total);
    let page = codeterm_plugin::jsonl_window::page_before(path, end, WINDOW_BYTES)
        .map_err(|error| StoredHistoryFailure::Unavailable.error(error))?;
    if page.end > 0 && page.start >= page.end {
        return Err("Stored history record exceeds backward scan limit".into());
    }
    let (bytes, start) = codeterm_plugin::jsonl_window::read_range(path, page.start, page.end)
        .map_err(|error| StoredHistoryFailure::Unavailable.error(error))?;
    let records = String::from_utf8(bytes).map_err(|error| StoredHistoryFailure::Corrupt.error(error))?;
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
    let Some(session) = tab
        .agent_session_id
        .as_deref()
        .filter(|session| !session.is_empty())
    else {
        return Ok(None);
    };
    let Some(stored) = read_stored_window(
        provider.as_ref(),
        tab.cwd.as_deref().unwrap_or_default(),
        session,
        before,
        crate::service::agent_runtime::engine::has_live_bound_session(&tab),
    )?
    else {
        return Ok(None);
    };
    let current = svc.mux.read().get_tab(tab_id).is_some_and(|current| {
        current.agent_type == tab.agent_type
            && current.agent_session_id == tab.agent_session_id
            && current.session_binding_nonce == tab.session_binding_nonce
    });
    if !current {
        return Err("Transcript binding changed during stored read".into());
    }
    log::debug!(
        "[chat-attach] tab={tab_id} phase=stored_snapshot elapsed_ms={} read_us={} projection_us={} bytes={} rows={}",
        stored.elapsed_ms,
        stored.read_us,
        stored.projection_us,
        stored.end.saturating_sub(stored.start),
        stored.messages.len()
    );
    Ok(Some(Window {
        messages: stored.messages,
        session: session.to_owned(),
        nonce: tab.session_binding_nonce,
        start: stored.start,
        end: stored.end,
        frozen: tab.frozen,
    }))
}

fn read_stored_window(
    provider: &dyn AgentProvider,
    cwd: &str,
    session: &str,
    before: Option<u64>,
    live_session_may_be_unmaterialized: bool,
) -> Result<Option<StoredWindow>, String> {
    if !provider
        .agent_runtime()
        .is_some_and(|capability| capability.stored_history)
        || !provider.runtime_stored_history_available(cwd, session)
    {
        return Ok(None);
    }
    let path = provider
        .session_file_path(cwd, session)
        .ok_or_else(|| StoredHistoryFailure::Missing.error("Provider has no recorded runtime session"))?;
    if !path.try_exists().map_err(|error| StoredHistoryFailure::Unavailable.error(error))? {
        return if live_session_may_be_unmaterialized {
            Ok(None)
        } else {
            Err(StoredHistoryFailure::Missing.error(format!("Stored runtime history is unavailable for session {session}")))
        };
    }
    let started = std::time::Instant::now();
    let input = if provider.record_source("storedHistory").is_some() {
        StoredInput::Records(super::jsonl_kernel::stored_window(
            provider,
            session,
            &path,
            before,
            WINDOW_BYTES,
        ).map_err(|error| StoredHistoryFailure::Corrupt.error(error))?)
    } else {
        let (records, start, end) = read_window(&path, before)
            .map_err(StoredHistoryFailure::projection)?;
        StoredInput::Text(records, start, end)
    };
    let read_us = started.elapsed().as_micros();
    let projecting = std::time::Instant::now();
    let (messages, start, end) = match input {
        StoredInput::Text(records, start, end) => (
            provider.runtime_stored_history(session, &records)
                .map_err(|error| StoredHistoryFailure::Corrupt.error(error))?,
            start,
            end,
        ),
        StoredInput::Records(window) => (
            provider.runtime_stored_records(session, &window)
                .map_err(|error| StoredHistoryFailure::Corrupt.error(error))?,
            window.meta.from_offset,
            window.meta.new_offset,
        ),
    };
    let projection_us = projecting.elapsed().as_micros();
    Ok(Some(StoredWindow {
        messages,
        start,
        end,
        elapsed_ms: started.elapsed().as_millis(),
        read_us,
        projection_us,
    }))
}

pub(super) fn archived_messages(
    svc: &ServiceContext,
    provider_id: &str,
    cwd: &str,
    session: &str,
) -> Result<Option<Vec<ChatMsg>>, String> {
    Ok(archived_window(svc, provider_id, cwd, session)?.map(|(messages, _)| messages))
}

pub(super) fn archived_window(
    svc: &ServiceContext,
    provider_id: &str,
    cwd: &str,
    session: &str,
) -> Result<Option<(Vec<ChatMsg>, u64)>, String> {
    let Some(provider) = svc.agent_registry.provider_arc(provider_id) else {
        return Ok(None);
    };
    if !provider
        .agent_runtime()
        .is_some_and(|capability| capability.stored_history)
    {
        return Ok(None);
    }
    let Some(window) = read_stored_window(provider.as_ref(), cwd, session, None, false)? else {
        let present = provider.session_file_path(cwd, session)
            .map(|path| path.try_exists())
            .transpose()
            .map_err(|error| StoredHistoryFailure::Unavailable.error(error))?
            .unwrap_or(false);
        let failure = if present { StoredHistoryFailure::Unavailable } else { StoredHistoryFailure::Missing };
        return Err(failure.error(format!(
            "Stored runtime history is unavailable for session {session}"
        )));
    };
    Ok(Some((window.messages, window.start)))
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
        archived_messages: None,
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
