use crate::service::ServiceState;

// Chat UI mode — thin Tauri wrappers over the tab_chat service.

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatOffsetKind {
    Bytes,
    Items,
}

#[derive(serde::Serialize)]
pub struct ChatStartResult {
    /// Vestigial raw-text channel; chat_start returns parsed `messages` today.
    pub text: String,
    pub offset_kind: ChatOffsetKind,
    pub start_offset: u64,
    pub offset: u64,
    /// Agent provisioning is in flight; the tab boot state supplies its visible
    /// progress until the mux reports a session id.
    #[serde(default)]
    pub pending: bool,
    /// Structured providers (sqlite-backed, e.g. opencode) return parsed
    /// messages here instead of raw `text`; the frontend renders them directly
    /// and listens on `tab-chat-msg-{id}` for deltas. `None` = JSONL provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<crate::agent::chat_types::ChatMsg>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub source_session_id: Option<String>,
    pub session_binding_nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,
    /// The provider named no transcript source for this bound session, so
    /// nothing is loading one: the chat view reports unavailable, never waits.
    pub source_missing: bool,
}

fn tab_chat_authority(svc: &ServiceState, tab_id: &str) -> (Option<String>, Option<String>) {
    svc.mux
        .read()
        .get_tab(tab_id)
        .map(|tab| {
            (
                tab.agent_session_id.clone(),
                tab.session_binding_nonce.clone(),
            )
        })
        .unwrap_or((None, None))
}

/// Shared by the Tauri command and the web mirror's
/// POST /api/tabs/{id}/chat-raw/start — identical result shape, so the
/// chat view renders the same in both shells.
/// Prepends the tab's pending orchestrator greeting (set on orchestrator
/// promotion) as a synthetic assistant `ChatMsg`, never model-generated.
/// Ordered first via a sentinel `seq` below any real transcript entry.
/// No-op (returns `messages` unchanged) when unset.
fn prepend_pending_greeting(
    svc: &ServiceState,
    tab_id: &str,
    messages: Vec<crate::agent::chat_types::ChatMsg>,
) -> Vec<crate::agent::chat_types::ChatMsg> {
    let greeting = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|p| p.pending_greeting.clone());
    let Some(content) = greeting else {
        return messages;
    };
    let mut out = Vec::with_capacity(messages.len() + 1);
    out.push(crate::agent::chat_types::ChatMsg {
        id: format!("greeting-{tab_id}"),
        kind: "assistant".into(),
        content,
        seq: i64::MIN,
        ..Default::default()
    });
    out.extend(messages);
    out
}

pub fn chat_start_core(
    svc: &ServiceState,
    tab_id: String,
    subscription_id: String,
) -> Result<ChatStartResult, String> {
    let mut result = chat_start_inner(svc, tab_id.clone(), subscription_id)?;
    if let Some(session_id) = result.source_session_id.as_deref() {
        if result
            .messages
            .as_ref()
            .is_some_and(|messages| messages.iter().any(|message| message.kind == "user"))
        {
            svc.note_session_conversation(&tab_id, session_id);
        }
    }
    crate::service::tab_chat::restore_notice::append_restore_notice(
        &tab_id,
        result.messages.get_or_insert_default(),
    );
    Ok(result)
}

fn chat_start_inner(
    svc: &ServiceState,
    tab_id: String,
    subscription_id: String,
) -> Result<ChatStartResult, String> {
    if crate::service::codex_rpc::lifecycle::for_tab(&tab_id).is_some() {
        return crate::service::codex_rpc::transcript::start(svc, &tab_id, subscription_id);
    }
    let started = std::time::Instant::now();
    let initial_authority = tab_chat_authority(svc, &tab_id);
    if crate::service::structured_endpoint::sse(&tab_id).is_some() && initial_authority.0.is_some()
    {
        let archives = svc
            .mux
            .read()
            .get_tab(&tab_id)
            .map(|tab| tab.context_epoch_archives.clone())
            .unwrap_or_default();
        let mut messages =
            super::context_epochs::materialize_archived_context_epochs(svc, &archives);
        messages.extend(crate::service::structured_prompt::snapshot::read(
            svc, &tab_id,
        )?);
        let start = messages
            .len()
            .saturating_sub(crate::service::structured_prompt::snapshot::RETAINED_ROWS);
        messages.drain(..start);
        if tab_chat_authority(svc, &tab_id) != initial_authority {
            return Err("Transcript binding changed during attach".into());
        }
        return Ok(ChatStartResult {
            text: String::new(),
            offset_kind: ChatOffsetKind::Bytes,
            start_offset: 0,
            offset: 0,
            pending: false,
            source_missing: false,
            messages: Some(prepend_pending_greeting(svc, &tab_id, messages)),
            cursor: None,
            source_session_id: initial_authority.0,
            session_binding_nonce: initial_authority.1,
            subscription_id: None,
        });
    }
    if let Some((plugin_id, _)) = svc.tab_chat_backing(&tab_id) {
        if svc.chatbackend_registry.get(&plugin_id).is_none() {
            return Err(crate::service::tab_transport_chat::removed_chatbackend_error(&plugin_id));
        }
        let (messages, _total) =
            crate::service::tab_chat::tab_chat_snapshot(svc, &tab_id, 0).unwrap_or_default();
        return Ok(ChatStartResult {
            text: String::new(),
            offset_kind: ChatOffsetKind::Bytes,
            start_offset: 0,
            offset: 0,
            pending: false,
            source_missing: false,
            messages: Some(prepend_pending_greeting(svc, &tab_id, messages)),
            cursor: None,
            source_session_id: initial_authority.0,
            session_binding_nonce: initial_authority.1,
            subscription_id: None,
        });
    }
    let (provider_id, cwd, session_id, context_epoch_archives, mux_ms) = {
        let t = std::time::Instant::now();
        let m = svc.mux.write();
        let tab = m.get_tab(&tab_id).ok_or("Tab not found")?;
        let Some(provider) = tab.agent_type.clone() else {
            if tab.agent_booting || tab.boot_stage.is_some() {
                return Ok(ChatStartResult {
                    text: String::new(),
                    offset_kind: ChatOffsetKind::Bytes,
                    start_offset: 0,
                    offset: 0,
                    pending: true,
                    source_missing: false,
                    messages: None,
                    cursor: None,
                    source_session_id: None,
                    session_binding_nonce: tab.session_binding_nonce.clone(),
                    subscription_id: None,
                });
            }
            return Err("Tab has no agent_type".into());
        };
        let cwd = tab.cwd.clone().unwrap_or_default();
        let Some(sid) = tab.agent_session_id.clone() else {
            if let Some(content) = tab.pending_greeting.clone() {
                return Ok(ChatStartResult {
                    text: String::new(),
                    offset_kind: ChatOffsetKind::Bytes,
                    start_offset: 0,
                    offset: 0,
                    pending: false,
                    source_missing: false,
                    messages: Some(vec![crate::agent::chat_types::ChatMsg {
                        id: format!("greeting-{tab_id}"),
                        kind: "assistant".into(),
                        content,
                        seq: i64::MIN,
                        ..Default::default()
                    }]),
                    cursor: None,
                    source_session_id: None,
                    session_binding_nonce: tab.session_binding_nonce.clone(),
                    subscription_id: None,
                });
            }
            return Ok(ChatStartResult {
                text: String::new(),
                offset_kind: ChatOffsetKind::Bytes,
                start_offset: 0,
                offset: 0,
                pending: true,
                source_missing: false,
                messages: None,
                cursor: None,
                source_session_id: None,
                session_binding_nonce: tab.session_binding_nonce.clone(),
                subscription_id: None,
            });
        };
        (
            provider,
            cwd,
            sid,
            tab.context_epoch_archives.clone(),
            t.elapsed().as_millis(),
        )
    };
    let archived_messages =
        super::context_epochs::materialize_archived_context_epochs(svc, &context_epoch_archives);
    let archive_available = !archived_messages.is_empty();

    let session_binding_nonce = svc
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.session_binding_nonce.clone());

    // structured messages in Rust and stream deltas from a DB poller.
    if svc
        .agent_registry
        .with_provider(&provider_id, |p| p.uses_structured_chat())
        .unwrap_or(false)
    {
        let delta = svc
            .agent_registry
            .with_provider(&provider_id, |p| {
                p.read_structured_chat(&cwd, &session_id, None)
            })
            .unwrap_or_default();
        if let Some(nonce) = session_binding_nonce.as_deref() {
            crate::service::provider_signals::apply_structured(
                &svc,
                &tab_id,
                &session_id,
                nonce,
                &delta.observations,
            );
        }
        let cursor = delta.cursor.clone();
        if !svc.chat_registry.acquire(
            &tab_id,
            &session_id,
            session_binding_nonce.as_deref(),
            &subscription_id,
        ) {
            let sink = crate::service::tab_chat::structured_sink::make_sink(
                svc.app_handle.clone(),
                svc.mux.clone(),
                Some(session_id.clone()),
                session_binding_nonce.clone(),
                Some(svc.chat_send_ledger.clone()),
            );
            let handle = crate::service::tab_chat::spawn_structured_poller(
                crate::service::tab_chat::StructuredPollConfig {
                    tab_id: tab_id.clone(),
                    provider_id: provider_id.clone(),
                    cwd: cwd.clone(),
                    session_id: session_id.clone(),
                    start_cursor: cursor.clone(),
                    poll_interval: crate::service::tab_chat::live_chat_poll_interval(),
                },
                svc.agent_registry.clone(),
                sink,
            );
            svc.chat_registry.insert(
                tab_id.clone(),
                session_id.clone(),
                session_binding_nonce.clone(),
                subscription_id.clone(),
                handle,
            );
        }
        let mut messages = archived_messages;
        messages.extend(delta.messages);
        return Ok(ChatStartResult {
            text: String::new(),
            offset_kind: ChatOffsetKind::Bytes,
            start_offset: 0,
            offset: 0,
            pending: false,
            source_missing: false,
            messages: Some(prepend_pending_greeting(svc, &tab_id, messages)),
            cursor,
            source_session_id: Some(session_id.clone()),
            session_binding_nonce: session_binding_nonce.clone(),
            subscription_id: Some(subscription_id),
        });
    }

    let path_started = std::time::Instant::now();
    let Some(path) = svc
        .agent_registry
        .with_provider(&provider_id, |p| p.session_jsonl_path(&cwd, &session_id))
        .flatten()
    else {
        log::warn!(
            "[chat_start] tab={} provider={} session={} has no JSONL yet",
            tab_id,
            provider_id,
            session_id
        );
        if !svc.chat_registry.acquire(
            &tab_id,
            &session_id,
            session_binding_nonce.as_deref(),
            &subscription_id,
        ) {
            let sink = crate::service::tab_chat::structured_sink::make_flatten_sink(
                svc.app_handle.clone(),
                svc.mux.clone(),
                0,
                Some(session_id.clone()),
                session_binding_nonce.clone(),
                Some(svc.chat_send_ledger.clone()),
            );
            let handle = crate::service::tab_chat::spawn_store_watch(
                crate::service::tab_chat::StoreWatchConfig {
                    tab_id: tab_id.clone(),
                    provider_id: provider_id.clone(),
                    cwd: cwd.clone(),
                    session_id: session_id.clone(),
                    session_binding_nonce: session_binding_nonce.clone(),
                    poll_interval:
                        crate::service::tab_chat::store_watch::PENDING_STORE_WATCH_POLL_INTERVAL,
                },
                svc.agent_registry.clone(),
                svc.chat_registry.clone(),
                sink,
            );
            handle.register(&svc.chat_registry, subscription_id.clone());
        }
        return Ok(ChatStartResult {
            text: String::new(),
            offset_kind: ChatOffsetKind::Bytes,
            start_offset: 0,
            offset: 0,
            pending: !archive_available,
            source_missing: true,
            messages: archive_available.then_some(archived_messages),
            cursor: None,
            source_session_id: Some(session_id.clone()),
            session_binding_nonce: session_binding_nonce.clone(),
            subscription_id: Some(subscription_id),
        });
    };
    let path_ms = path_started.elapsed().as_millis();

    let read_started = std::time::Instant::now();
    let snap = crate::service::tab_chat::session_window::snapshot(
        &svc.agent_registry,
        &provider_id,
        &path,
    );
    // Scope carries the attach that asked, so a second bounded line in the log
    // is decidable: a new subscription is a re-attach, a repeat is a double read.
    crate::service::tab_chat::session_window::log_bounded_snapshot(
        &format!("tab:{tab_id}/{subscription_id}"),
        &provider_id,
        &snap,
    );
    let read_ms = read_started.elapsed().as_millis();
    if let Some(nonce) = session_binding_nonce.as_deref() {
        if snap.truncated() {
            crate::service::background_work::incomplete_snapshot(svc, &tab_id, &session_id, nonce);
        }
        crate::service::provider_signals::apply(
            svc,
            &tab_id,
            &session_id,
            nonce,
            &snap.runtime_observations,
        );
        crate::service::background_work::apply(
            svc,
            &tab_id,
            &session_id,
            nonce,
            &snap.runtime_observations,
        );
    }
    crate::api::hook_routes::context_feed::apply_snapshot(
        svc,
        &tab_id,
        &session_id,
        session_binding_nonce.as_deref(),
        snap.context_usage.as_ref(),
    );
    // Resume at the parsed end, not the file size: a held-back partial record
    // must be consumed once by the tailer rather than skipped.
    let (total, tailer_start_offset, bytes_read) = (snap.total, snap.end_offset, snap.bytes_read);
    // Backfill cursor: >0 means the bounded snapshot truncated older history.
    let snapshot_start = snap.start_offset;
    let msg_count = snap.messages.len();
    let mut messages = archived_messages;
    messages.extend(snap.messages);

    // is already running so one client's stop() cannot kill another's stream.
    let tailer_started = std::time::Instant::now();
    if !svc.chat_registry.acquire(
        &tab_id,
        &session_id,
        session_binding_nonce.as_deref(),
        &subscription_id,
    ) {
        let sink = crate::service::tab_chat::structured_sink::make_flatten_sink(
            svc.app_handle.clone(),
            svc.mux.clone(),
            msg_count,
            Some(session_id.clone()),
            session_binding_nonce.clone(),
            Some(svc.chat_send_ledger.clone()),
        );
        let handle = crate::service::tab_chat::tailer::spawn_sourced_tailer(
            crate::service::tab_chat::TailConfig {
                tab_id: tab_id.clone(),
                provider_id: provider_id.clone(),
                cwd: cwd.clone(),
                session_id: session_id.clone(),
                start_offset: tailer_start_offset,
            },
            Some(svc.chat_registry.jsonl_source(&tab_id)),
            svc.agent_registry.clone(),
            sink,
        );
        svc.chat_registry.insert(
            tab_id.clone(),
            session_id.clone(),
            session_binding_nonce.clone(),
            subscription_id.clone(),
            handle,
        );
    }
    let tailer_ms = tailer_started.elapsed().as_millis();
    let total_ms = started.elapsed().as_millis();
    if total_ms > 250 {
        log::warn!(
            "[chat_start.slow] tab={} provider={} total_ms={} mux={} path={} read={} tailer={} total_bytes={} read_bytes={} messages={}",
            tab_id,
            provider_id,
            total_ms,
            mux_ms,
            path_ms,
            read_ms,
            tailer_ms,
            total,
            bytes_read,
            msg_count
        );
    }

    Ok(ChatStartResult {
        // `[start_offset, offset)` is the record-bounded snapshot range the
        // client reconciles from; it pages older below start_offset.
        offset_kind: ChatOffsetKind::Bytes,
        start_offset: snapshot_start,
        text: String::new(),
        offset: tailer_start_offset,
        pending: false,
        source_missing: false,
        messages: Some(prepend_pending_greeting(svc, &tab_id, messages)),
        cursor: None,
        source_session_id: Some(session_id),
        session_binding_nonce,
        subscription_id: Some(subscription_id),
    })
}

#[derive(serde::Serialize)]
pub struct ChatBackfillResult {
    pub text: String,
    pub offset_kind: ChatOffsetKind,
    pub start_offset: u64,
    pub end_offset: u64,
    /// Window parsed with the tab's own provider parser — same normalized
    /// shape as `ChatStartResult::messages`, ready to merge client-side.
    pub messages: Vec<crate::agent::chat_types::ChatMsg>,
    pub source_session_id: Option<String>,
    pub session_binding_nonce: Option<String>,
}

/// Shared by the Tauri command and the web mirror's
/// POST /api/tabs/{id}/chat-raw/backfill.
pub fn chat_backfill_core(
    svc: &ServiceState,
    tab_id: String,
    before_offset: u64,
) -> Result<ChatBackfillResult, String> {
    if crate::service::codex_rpc::lifecycle::for_tab(&tab_id).is_some() {
        return crate::service::codex_rpc::transcript::backfill(svc, &tab_id, before_offset);
    }
    let (provider_id, cwd, session_id, session_binding_nonce) = {
        let m = svc.mux.write();
        let tab = m.get_tab(&tab_id).ok_or("Tab not found")?;
        (
            tab.agent_type.clone().ok_or("no agent_type")?,
            tab.cwd.clone().unwrap_or_default(),
            tab.agent_session_id.clone().ok_or("no session_id")?,
            tab.session_binding_nonce.clone(),
        )
    };
    let path = svc
        .agent_registry
        .with_provider(&provider_id, |p| p.session_jsonl_path(&cwd, &session_id))
        .flatten()
        .ok_or("no JSONL")?;

    Ok(ChatBackfillResult {
        source_session_id: Some(session_id),
        session_binding_nonce,
        ..backfill_page(&svc.agent_registry, &provider_id, &path, before_offset)?
    })
}

/// One backfill page of `path` ending at `before_offset`, without session scope.
pub(crate) fn backfill_page(
    registry: &std::sync::Arc<crate::agent::registry::AgentRegistry>,
    provider_id: &str,
    path: &std::path::Path,
    before_offset: u64,
) -> Result<ChatBackfillResult, String> {
    let page_bytes = crate::service::tab_chat::session_window::BACKFILL_PAGE_BYTES;
    let (text, aligned_start) = backfill_window_slice(path, before_offset, page_bytes)
        .map_err(|e| format!("read jsonl: {e}"))?;
    let page = crate::service::tab_chat::session_window::parse_page_before(
        registry,
        provider_id,
        path,
        before_offset,
    );
    Ok(ChatBackfillResult {
        text,
        // `aligned_start` is the first complete record read, so the next page
        // ends there and parses the partial head this read dropped.
        offset_kind: ChatOffsetKind::Bytes,
        start_offset: aligned_start,
        end_offset: page.end_offset,
        messages: crate::service::tab_chat::rich_rows_to_chat_msgs(&page.messages, aligned_start),
        source_session_id: None,
        session_binding_nonce: None,
    })
}

/// Seek one backfill page and align it forward to the first complete record.
/// A read already on a boundary keeps it, so no emitted record falls outside.
fn backfill_window_slice(
    path: &std::path::Path,
    before_offset: u64,
    max_bytes: u64,
) -> Result<(String, u64), String> {
    let start = before_offset.saturating_sub(max_bytes);
    // Seek the requested range only — the file behind it can be hundreds of MB.
    let (window, clamped_start) =
        codeterm_plugin::jsonl_window::read_range(path, start, before_offset)
            .map_err(|e| e.to_string())?;
    let already_aligned = clamped_start == 0 || record_starts_at(path, clamped_start);
    let aligned_offset = if already_aligned {
        0usize
    } else {
        window
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    };
    let text = String::from_utf8_lossy(&window[aligned_offset.min(window.len())..]).to_string();
    Ok((text, clamped_start + aligned_offset as u64))
}

/// True when `offset` is itself a record start (the byte before it terminates
/// the previous record).
fn record_starts_at(path: &std::path::Path, offset: u64) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    if file.seek(SeekFrom::Start(offset - 1)).is_err() {
        return false;
    }
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).is_ok() && byte[0] == b'\n'
}

#[cfg(test)]
mod tests;
