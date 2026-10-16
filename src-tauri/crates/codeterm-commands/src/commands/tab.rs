use codeterm_service_core::service::tab_chat::results::{
    ChatBackfillResult, ChatOffsetKind, ChatStartResult,
};
use codeterm_service_core::service::ServiceState;

// Chat UI mode — thin Tauri wrappers over the tab_chat service.

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
    messages: Vec<codeterm_service_core::agent::chat_types::ChatMsg>,
) -> Vec<codeterm_service_core::agent::chat_types::ChatMsg> {
    let greeting = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|p| p.pending_greeting.clone());
    let Some(content) = greeting else {
        return messages;
    };
    let mut out = Vec::with_capacity(messages.len() + 1);
    out.push(codeterm_service_core::agent::chat_types::ChatMsg {
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
    let started = std::time::Instant::now();
    let mut result = chat_start_inner(svc, tab_id.clone(), subscription_id)?;
    log::debug!(
        "[chat-attach] tab={tab_id} phase=inner_result elapsed_ms={}",
        started.elapsed().as_millis()
    );
    let evidence_at = std::time::Instant::now();
    if let Some(session_id) = result.source_session_id.as_deref() {
        if result
            .messages
            .as_ref()
            .is_some_and(|messages| messages.iter().any(|message| message.kind == "user"))
        {
            svc.note_session_conversation(&tab_id, session_id);
        }
    }
    log::debug!(
        "[chat-attach] tab={tab_id} phase=conversation_evidence elapsed_ms={}",
        evidence_at.elapsed().as_millis()
    );
    let notice_at = std::time::Instant::now();
    codeterm_service_core::service::tab_chat::append_restore_notice(
        &svc.lifecycle_journal,
        &tab_id,
        result.messages.get_or_insert_default(),
    );
    log::debug!(
        "[chat-attach] tab={tab_id} phase=restore_notice elapsed_ms={} total_ms={}",
        notice_at.elapsed().as_millis(),
        started.elapsed().as_millis()
    );
    Ok(result)
}

fn chat_start_inner(
    svc: &ServiceState,
    tab_id: String,
    subscription_id: String,
) -> Result<ChatStartResult, String> {
    let attached_at = std::time::Instant::now();
    let runtime_requested =
        codeterm_service_core::service::agent_runtime::requested_for_tab(svc, &tab_id);
    log::debug!(
        "[chat-attach] tab={tab_id} phase=capability elapsed_ms={} runtime={runtime_requested}",
        attached_at.elapsed().as_millis()
    );
    if runtime_requested {
        if let Some(snapshot) = codeterm_service_core::service::tab_chat::runtime_stored::start(
            svc,
            &tab_id,
            &subscription_id,
        )? {
            return Ok(snapshot);
        }
        let (session, messages) =
            codeterm_service_core::service::agent_runtime::transport::snapshot(svc, &tab_id)?;
        let context = session.context.lock().clone();
        let total = messages.len();
        let start = total.saturating_sub(200);
        log::debug!(
            "[chat-attach] tab={tab_id} phase=runtime_snapshot elapsed_ms={} rows={total}",
            attached_at.elapsed().as_millis()
        );
        return Ok(ChatStartResult {
            text: String::new(),
            offset_kind: ChatOffsetKind::Items,
            start_offset: start as u64,
            offset: total as u64,
            pending: false,
            messages: Some(messages.into_iter().skip(start).collect()),
            cursor: None,
            source_session_id: context.thread_id,
            session_binding_nonce: Some(context.binding_nonce),
            subscription_id: Some(subscription_id),
            source_missing: false,
        });
    }
    if codeterm_service_core::service::codex_rpc::lifecycle::for_tab(&tab_id).is_some() {
        return codeterm_service_core::service::codex_rpc::transcript::start(
            svc,
            &tab_id,
            subscription_id,
        );
    }
    let started = std::time::Instant::now();
    let initial_authority = tab_chat_authority(svc, &tab_id);
    if codeterm_service_core::service::structured_endpoint_sse(&tab_id).is_some()
        && initial_authority.0.is_some()
    {
        let archives = svc
            .mux
            .read()
            .get_tab(&tab_id)
            .map(|tab| tab.context_epoch_archives.clone())
            .unwrap_or_default();
        let mut messages =
            super::context_epochs::materialize_archived_context_epochs(svc, &archives);
        messages.extend(
            codeterm_service_core::service::structured_prompt::read_snapshot(svc, &tab_id)?,
        );
        let start = messages
            .len()
            .saturating_sub(codeterm_service_core::service::structured_prompt::RETAINED_ROWS);
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
            return Err(
                codeterm_service_core::service::tab_transport_chat::removed_chatbackend_error(
                    &plugin_id,
                ),
            );
        }
        let (messages, _total) =
            codeterm_service_core::service::tab_chat::tab_chat_snapshot(svc, &tab_id, 0)
                .unwrap_or_default();
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
        let m = svc.mux.read();
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
                    messages: Some(vec![codeterm_service_core::agent::chat_types::ChatMsg {
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
            codeterm_service_core::service::provider_signals_apply_structured(
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
            let sink = codeterm_service_core::service::tab_chat::structured_sink::make_sink(
                svc.lifecycle_journal.clone(),
                &svc,
                svc.mux.clone(),
                Some(session_id.clone()),
                session_binding_nonce.clone(),
                Some(svc.chat_send_ledger.clone()),
            );
            let handle = codeterm_service_core::service::tab_chat::spawn_structured_poller(
                codeterm_service_core::service::tab_chat::StructuredPollConfig {
                    tab_id: tab_id.clone(),
                    provider_id: provider_id.clone(),
                    cwd: cwd.clone(),
                    session_id: session_id.clone(),
                    start_cursor: cursor.clone(),
                    poll_interval:
                        codeterm_service_core::service::tab_chat::live_chat_poll_interval(),
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
            let sink = codeterm_service_core::service::tab_chat::structured_sink::make_flatten_sink(
                svc.lifecycle_journal.clone(),
                &svc,
                svc.mux.clone(),
                0,
                Some(session_id.clone()),
                session_binding_nonce.clone(),
                Some(svc.chat_send_ledger.clone()),
            );
            let handle = codeterm_service_core::service::tab_chat::spawn_store_watch(
                codeterm_service_core::service::tab_chat::StoreWatchConfig {
                    tab_id: tab_id.clone(),
                    provider_id: provider_id.clone(),
                    cwd: cwd.clone(),
                    session_id: session_id.clone(),
                    session_binding_nonce: session_binding_nonce.clone(),
                    poll_interval:
                        codeterm_service_core::service::tab_chat::store_watch::PENDING_STORE_WATCH_POLL_INTERVAL,
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

    let source = svc.chat_registry.jsonl_source(&tab_id);
    let resume_epoch = codeterm_service_core::service::tab_chat::resume_cursor::ChatEpoch {
        source_session_id: session_id.clone(),
        session_binding_nonce: session_binding_nonce.clone(),
    };
    let incarnation = codeterm_service_core::service::tab_chat::resume::capture_incarnation(
        &source,
        &resume_epoch,
        &path,
        0,
        None,
    );
    let read_started = std::time::Instant::now();
    let snap = svc.chat_registry.snapshot_flights.run(
        codeterm_service_core::service::tab_chat::snapshot_flight::SnapshotKey {
            tab_id: tab_id.clone(),
            provider_id: provider_id.clone(),
            session_id: session_id.clone(),
            binding_nonce: session_binding_nonce.clone(),
            path: path.clone(),
            incarnation: incarnation.as_ref().map(|current| current.id.to_string()),
        },
        || {
            codeterm_service_core::service::tab_chat::session_window::snapshot(
                &svc.agent_registry,
                &provider_id,
                &path,
            )
        },
    )?;
    // Scope carries the attach that asked, so a second bounded line in the log
    // is decidable: a new subscription is a re-attach, a repeat is a double read.
    codeterm_service_core::service::tab_chat::session_window::log_bounded_snapshot(
        &format!("tab:{tab_id}/{subscription_id}"),
        &provider_id,
        &snap,
    );
    let cursor = incarnation.as_ref().and_then(|held| {
        codeterm_service_core::service::tab_chat::resume::capture_incarnation(
            &source,
            &resume_epoch,
            &path,
            snap.end_offset,
            Some(held),
        )
        .and_then(|current| {
            codeterm_service_core::service::tab_chat::resume::encode_cursor(
                resume_epoch,
                &current,
                snap.end_offset,
            )
        })
    });
    let read_ms = read_started.elapsed().as_millis();
    if let Some(nonce) = session_binding_nonce.as_deref() {
        if snap.truncated() {
            codeterm_service_core::service::incomplete_snapshot(svc, &tab_id, &session_id, nonce);
        }
        codeterm_service_core::service::provider_signals_apply(
            svc,
            &tab_id,
            &session_id,
            nonce,
            &snap.runtime_observations,
        );
        codeterm_service_core::service::background_work_apply(
            svc,
            &tab_id,
            &session_id,
            nonce,
            &snap.runtime_observations,
        );
    }
    codeterm_service_core::service::context_feed::apply_snapshot(
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
    messages.extend(snap.messages.iter().cloned());

    // is already running so one client's stop() cannot kill another's stream.
    let tailer_started = std::time::Instant::now();
    if !svc.chat_registry.acquire(
        &tab_id,
        &session_id,
        session_binding_nonce.as_deref(),
        &subscription_id,
    ) {
        let sink = codeterm_service_core::service::tab_chat::structured_sink::make_flatten_sink(
            svc.lifecycle_journal.clone(),
            &svc,
            svc.mux.clone(),
            msg_count,
            Some(session_id.clone()),
            session_binding_nonce.clone(),
            Some(svc.chat_send_ledger.clone()),
        );
        let handle = codeterm_service_core::service::tab_chat::tailer::spawn_sourced_tailer(
            codeterm_service_core::service::tab_chat::TailConfig {
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
        cursor,
        source_session_id: Some(session_id),
        session_binding_nonce,
        subscription_id: Some(subscription_id),
    })
}

/// Shared by the Tauri command and the web mirror's
/// POST /api/tabs/{id}/chat-raw/backfill.
pub fn chat_backfill_core(
    svc: &ServiceState,
    tab_id: String,
    before_offset: u64,
    offset_kind: Option<ChatOffsetKind>,
) -> Result<ChatBackfillResult, String> {
    if matches!(offset_kind, Some(ChatOffsetKind::Bytes)) {
        if let Some(page) = codeterm_service_core::service::tab_chat::runtime_stored::backfill(
            svc,
            &tab_id,
            before_offset,
        )? {
            return Ok(page);
        }
    }
    if codeterm_service_core::service::agent_runtime::requested_for_tab(svc, &tab_id) {
        let (session, messages) =
            codeterm_service_core::service::agent_runtime::transport::snapshot(svc, &tab_id)?;
        let context = session.context.lock().clone();
        let end = usize::try_from(before_offset)
            .unwrap_or(usize::MAX)
            .min(messages.len());
        let start = end.saturating_sub(200);
        return Ok(ChatBackfillResult {
            text: String::new(),
            offset_kind: ChatOffsetKind::Items,
            start_offset: start as u64,
            end_offset: end as u64,
            messages: messages.into_iter().skip(start).take(end - start).collect(),
            source_session_id: context.thread_id,
            session_binding_nonce: Some(context.binding_nonce),
        });
    }
    if codeterm_service_core::service::codex_rpc::lifecycle::for_tab(&tab_id).is_some() {
        return codeterm_service_core::service::codex_rpc::transcript::backfill(
            svc,
            &tab_id,
            before_offset,
        );
    }
    let (provider_id, cwd, session_id, session_binding_nonce) = {
        let m = svc.mux.read();
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
    registry: &std::sync::Arc<codeterm_service_core::agent::registry::AgentRegistry>,
    provider_id: &str,
    path: &std::path::Path,
    before_offset: u64,
) -> Result<ChatBackfillResult, String> {
    let page = codeterm_service_core::service::tab_chat::session_window::parse_page_before(
        registry,
        provider_id,
        path,
        before_offset,
    )?;
    Ok(ChatBackfillResult {
        text: String::new(),
        offset_kind: ChatOffsetKind::Bytes,
        start_offset: page.start_offset,
        end_offset: page.end_offset,
        messages: codeterm_service_core::service::tab_chat::rich_rows_to_chat_msgs(
            &page.messages,
            page.start_offset,
        ),
        source_session_id: None,
        session_binding_nonce: None,
    })
}

/// Seek one backfill page and align it forward to the first complete record.
/// A read already on a boundary keeps it, so no emitted record falls outside.
#[cfg(test)]
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
#[cfg(test)]
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
