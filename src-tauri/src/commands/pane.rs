use crate::multiplexer::commands::MuxState;
use crate::service::ServiceState;
use crate::ScreenStateHandle;

// NOTE: keep `RichMessage`/`SessionDelta` and `spawn_tailer`/`DeltaSink`
// for future non-UI consumers. Frontend uses raw-text tailer + TS parser today.

#[tauri::command]
pub async fn get_pane_output(
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
) -> Result<String, String> {
    let cmd = crate::dispatch::Command::GetOutput {
        pane_id,
        offset: None,
        max_bytes: None,
        strip_ansi: true,
        full: true,
    };
    let result = crate::dispatch::dispatch_blocking(cmd, svc.inner().clone()).await?;
    Ok(result
        .into_json()
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string())
}

/// Raw PTY history with ANSI intact — frontend feeds it back to xterm on remount.
#[tauri::command]
pub async fn get_pane_history_raw(
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
) -> Result<String, String> {
    let cmd = crate::dispatch::Command::GetOutput {
        pane_id,
        offset: None,
        max_bytes: None,
        strip_ansi: false,
        full: true,
    };
    let result = crate::dispatch::dispatch_blocking(cmd, svc.inner().clone()).await?;
    Ok(result
        .into_json()
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string())
}

/// formatted=true preserves ANSI + cursor state (needed for TUI remount).
#[tauri::command]
pub async fn get_pane_text(
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
    formatted: Option<bool>,
) -> Result<String, String> {
    let cmd = crate::dispatch::Command::GetText {
        pane_id,
        formatted: formatted.unwrap_or(false),
    };
    let result = crate::dispatch::dispatch_blocking(cmd, svc.inner().clone()).await?;
    Ok(result
        .into_json()
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string())
}

/// Shell-integration snapshot for a pane: the accumulated command list + cwd
/// the `shell-command-changed` events stream live. OSC4 chat hydrates from this
/// on mount so existing commands render, not only ones seen after subscribing.
#[tauri::command]
pub async fn shell_command_snapshot(
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
) -> Result<serde_json::Value, String> {
    let pty_id = svc.resolve_pty_id(&pane_id).ok();
    let snap = pty_id.and_then(|pty_id| svc.shell_caps.lock().snapshot(&pty_id));
    serde_json::to_value(snap.unwrap_or_default()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_agent_providers(
    svc: tauri::State<'_, ServiceState>,
) -> Result<serde_json::Value, String> {
    let result =
        crate::dispatch::dispatch(&crate::dispatch::Command::ListAgentProviders, svc.as_ref())?;
    Ok(result.into_json())
}

/// Orchestrator-side remote (cross-mesh) workers, as `{ pane_id: [...] }`.
/// Drives the "remote pane" ghost nodes in the sidebar; live updates arrive via
/// the `remote-workers-changed` event.
#[tauri::command]
pub async fn get_remote_workers(
    svc: tauri::State<'_, ServiceState>,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(svc.remote_workers.snapshot()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn wait_shell_ready(
    screen_state: tauri::State<'_, ScreenStateHandle>,
    mux: tauri::State<'_, MuxState>,
    pane_id: String,
    timeout_ms: Option<u64>,
) -> Result<bool, String> {
    let pty_id = {
        let m = mux.write();
        m.get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
            .ok_or_else(|| "Pane not found or has no PTY".to_string())?
    };
    let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(10_000));
    let ss = screen_state.inner().clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::service::pty_io::poll_shell_prompt(&ss, &pty_id, timeout)
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(result)
}

#[tauri::command]
pub fn read_session_jsonl(
    mux: tauri::State<'_, MuxState>,
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
    offset: Option<u64>,
    tail_bytes: Option<u64>,
) -> Result<String, String> {
    let (session_id, cwd, agent_type) = {
        let m = mux.write();
        let pane = m
            .get_pane(&pane_id)
            .ok_or_else(|| "Pane not found".to_string())?;
        let sid = pane.agent_session_id.clone();
        let cwd = pane
            .cwd
            .clone()
            .ok_or_else(|| "Pane has no CWD".to_string())?;
        let at = pane
            .agent_type
            .clone()
            .unwrap_or_else(crate::config::default_agent_provider);
        (sid, cwd, at)
    };

    let session_id =
        session_id.ok_or_else(|| "No agent session linked to this pane".to_string())?;

    let jsonl_path = svc
        .agent_registry
        .with_provider(&agent_type, |p| p.session_file_path(&cwd, &session_id))
        .flatten()
        .ok_or_else(|| format!("No session file path for agent type: {}", agent_type))?;

    if !jsonl_path.exists() {
        return Err(format!("JSONL not found: {}", jsonl_path.display()));
    }

    use std::io::{Read as IoRead, Seek, SeekFrom};
    let mut file = std::fs::File::open(&jsonl_path).map_err(|e| e.to_string())?;
    let file_len = file.metadata().map_err(|e| e.to_string())?.len();

    let skip = offset.unwrap_or(0);
    if skip > 0 {
        if skip >= file_len {
            return Ok(String::new());
        }
        file.seek(SeekFrom::Start(skip))
            .map_err(|e| e.to_string())?;
        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    } else if let Some(tail) = tail_bytes {
        let start = file_len.saturating_sub(tail);
        file.seek(SeekFrom::Start(start))
            .map_err(|e| e.to_string())?;
        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        if start > 0 {
            if let Some(pos) = buf.find('\n') {
                buf = buf[pos + 1..].to_string();
            }
        }
        Ok(format!("__offset:{}\n{}", file_len, buf))
    } else {
        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }
}

// Chat UI mode — thin Tauri wrappers over the pane_chat service.

#[derive(serde::Serialize)]
pub struct ChatStartResult {
    /// Raw JSONL text from the recent window — frontend parses via the
    /// claude TS plugin's `parseJsonlSession`. The window is anchored to
    /// a `\n` boundary so the parser never sees a partial first line.
    pub text: String,
    pub start_offset: u64,
    pub offset: u64,
    /// Agent pane exists but its session hasn't been created yet — render an empty chat and retry once the mux reports a session id.
    #[serde(default)]
    pub pending: bool,
    /// Structured providers (sqlite-backed, e.g. opencode) return parsed
    /// messages here instead of raw `text`; the frontend renders them directly
    /// and listens on `pane-chat-msg-{id}` for deltas. `None` = JSONL provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<crate::agent::chat_types::ChatMsg>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub source_session_id: Option<String>,
    pub session_binding_nonce: Option<String>,
}

fn pane_chat_authority(svc: &ServiceState, pane_id: &str) -> (Option<String>, Option<String>) {
    svc.mux
        .read()
        .get_pane(pane_id)
        .map(|pane| {
            (
                pane.agent_session_id.clone(),
                pane.session_binding_nonce.clone(),
            )
        })
        .unwrap_or((None, None))
}

#[tauri::command]
pub async fn chat_start(
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
) -> Result<ChatStartResult, String> {
    let svc_owned = svc.inner().clone();
    tokio::task::spawn_blocking(move || chat_start_core(&svc_owned, pane_id))
        .await
        .map_err(|e| e.to_string())?
}

/// Shared by the Tauri command and the web mirror's
/// POST /api/panes/{id}/chat-raw/start — identical result shape, so the
/// chat view renders the same in both shells.
/// Prepends the pane's pending orchestrator greeting (set on orchestrator
/// promotion) as a synthetic assistant `ChatMsg`, never model-generated.
/// Ordered first via a sentinel `seq` below any real transcript entry.
/// No-op (returns `messages` unchanged) when unset.
fn prepend_pending_greeting(
    svc: &ServiceState,
    pane_id: &str,
    messages: Vec<crate::agent::chat_types::ChatMsg>,
) -> Vec<crate::agent::chat_types::ChatMsg> {
    let greeting = svc
        .mux
        .read()
        .get_pane(pane_id)
        .and_then(|p| p.pending_greeting.clone());
    let Some(content) = greeting else {
        return messages;
    };
    let mut out = Vec::with_capacity(messages.len() + 1);
    out.push(crate::agent::chat_types::ChatMsg {
        id: format!("greeting-{pane_id}"),
        kind: "assistant".into(),
        content,
        seq: i64::MIN,
        ..Default::default()
    });
    out.extend(messages);
    out
}

pub fn chat_start_core(svc: &ServiceState, pane_id: String) -> Result<ChatStartResult, String> {
    let started = std::time::Instant::now();
    let initial_authority = pane_chat_authority(svc, &pane_id);
    if svc.pane_chat_backing(&pane_id).is_some() {
        let (messages, _total) =
            crate::service::pane_chat::pane_chat_snapshot(svc, &pane_id, 0).unwrap_or_default();
        return Ok(ChatStartResult {
            text: String::new(),
            start_offset: 0,
            offset: 0,
            pending: false,
            messages: Some(prepend_pending_greeting(svc, &pane_id, messages)),
            cursor: None,
            source_session_id: initial_authority.0,
            session_binding_nonce: initial_authority.1,
        });
    }
    let (provider_id, cwd, mut session_id, mux_ms) = {
        let t = std::time::Instant::now();
        let m = svc.mux.write();
        let pane = m.get_pane(&pane_id).ok_or("pane not found")?;
        let provider = pane.agent_type.clone().ok_or("pane has no agent_type")?;
        let cwd = pane.cwd.clone().unwrap_or_default();
        let Some(sid) = pane.agent_session_id.clone() else {
            if let Some(content) = pane.pending_greeting.clone() {
                return Ok(ChatStartResult {
                    text: String::new(),
                    start_offset: 0,
                    offset: 0,
                    pending: false,
                    messages: Some(vec![crate::agent::chat_types::ChatMsg {
                        id: format!("greeting-{pane_id}"),
                        kind: "assistant".into(),
                        content,
                        seq: i64::MIN,
                        ..Default::default()
                    }]),
                    cursor: None,
                    source_session_id: None,
                    session_binding_nonce: pane.session_binding_nonce.clone(),
                });
            }
            return Ok(ChatStartResult {
                text: String::new(),
                start_offset: 0,
                offset: 0,
                pending: true,
                messages: None,
                cursor: None,
                source_session_id: None,
                session_binding_nonce: pane.session_binding_nonce.clone(),
            });
        };
        (provider, cwd, sid, t.elapsed().as_millis())
    };

    // to a newer backing session (Codex resume starts a fresh rollout). Only accept that provider-owned handoff when no sibling pane already owns it.
    let detect_started = std::time::Instant::now();
    if let Some(active_sid) = svc
        .agent_registry
        .with_provider(&provider_id, |p| p.detect_active_session(&cwd, &session_id))
        .flatten()
    {
        let claimed = {
            let mut mux = svc.mux.write();
            if active_sid == session_id || mux.other_session_ids(&pane_id).contains(&active_sid) {
                false
            } else if let Some(pane) = mux.get_pane_mut(&pane_id) {
                if pane.agent_type.as_deref() == Some(provider_id.as_str()) {
                    pane.agent_session_id = Some(active_sid.clone());
                    crate::service::pane::clear_runtime_model_evidence(pane);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if claimed {
            log::info!(
                "[chat_start] rolled pane {} from {} -> {} ({})",
                pane_id,
                session_id,
                active_sid,
                provider_id,
            );
            svc.emit_state_changed();
            session_id = active_sid;
        }
    }
    let detect_ms = detect_started.elapsed().as_millis();
    let session_binding_nonce = svc
        .mux
        .read()
        .get_pane(&pane_id)
        .and_then(|pane| pane.session_binding_nonce.clone());

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
        let cursor = delta.cursor.clone();
        if !svc.chat_registry.acquire(&pane_id, &session_id) {
            let sink = crate::service::pane_chat::structured_sink::make_sink(
                svc.app_handle.clone(),
                Some(session_id.clone()),
                session_binding_nonce.clone(),
            );
            let handle = crate::service::pane_chat::spawn_structured_poller(
                crate::service::pane_chat::StructuredPollConfig {
                    pane_id: pane_id.clone(),
                    provider_id: provider_id.clone(),
                    cwd: cwd.clone(),
                    session_id: session_id.clone(),
                    start_cursor: cursor.clone(),
                    poll_interval: crate::service::pane_chat::live_chat_poll_interval(),
                },
                svc.agent_registry.clone(),
                sink,
            );
            svc.chat_registry
                .insert(pane_id.clone(), session_id.clone(), handle);
        }
        return Ok(ChatStartResult {
            text: String::new(),
            start_offset: 0,
            offset: 0,
            pending: false,
            messages: Some(prepend_pending_greeting(svc, &pane_id, delta.messages)),
            cursor,
            source_session_id: Some(session_id.clone()),
            session_binding_nonce: session_binding_nonce.clone(),
        });
    }

    let path_started = std::time::Instant::now();
    let Some(path) = svc
        .agent_registry
        .with_provider(&provider_id, |p| p.session_jsonl_path(&cwd, &session_id))
        .flatten()
    else {
        log::warn!(
            "[chat_start] pane={} provider={} session={} has no JSONL yet",
            pane_id,
            provider_id,
            session_id
        );
        if !svc.chat_registry.acquire(&pane_id, &session_id) {
            let sink = crate::service::pane_chat::structured_sink::make_flatten_sink(
                svc.app_handle.clone(),
                0,
                Some(session_id.clone()),
                session_binding_nonce.clone(),
            );
            let handle = crate::service::pane_chat::spawn_store_watch(
                crate::service::pane_chat::StoreWatchConfig {
                    pane_id: pane_id.clone(),
                    provider_id: provider_id.clone(),
                    cwd: cwd.clone(),
                    session_id: session_id.clone(),
                    poll_interval:
                        crate::service::pane_chat::store_watch::PENDING_STORE_WATCH_POLL_INTERVAL,
                },
                svc.agent_registry.clone(),
                svc.chat_registry.clone(),
                sink,
            );
            handle.register(&svc.chat_registry);
        }
        return Ok(ChatStartResult {
            text: String::new(),
            start_offset: 0,
            offset: 0,
            pending: true,
            messages: None,
            cursor: None,
            source_session_id: Some(session_id.clone()),
            session_binding_nonce: session_binding_nonce.clone(),
        });
    };
    let total = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let path_ms = path_started.elapsed().as_millis();

    const JSONL_SNAPSHOT_CAP: u64 = 10 * 1024 * 1024;
    const JSONL_SNAPSHOT_WINDOW: u64 = 4 * 1024 * 1024;
    let read_started = std::time::Instant::now();
    let (messages, msg_count, tailer_start_offset) = if total > JSONL_SNAPSHOT_CAP {
        let start = jsonl_tail_window_start(&path, total, JSONL_SNAPSHOT_WINDOW);
        let msgs = jsonl_snapshot_from_offset(&svc.agent_registry, &provider_id, &path, start);
        let count = msgs.len();
        (msgs, count, total)
    } else {
        let (msgs, count) =
            crate::service::pane_chat::pane_chat_snapshot(svc, &pane_id, 0).unwrap_or_default();
        (msgs, count, total)
    };
    let read_ms = read_started.elapsed().as_millis();

    // is already running so one client's stop() cannot kill another's stream.
    let tailer_started = std::time::Instant::now();
    if !svc.chat_registry.acquire(&pane_id, &session_id) {
        let sink = crate::service::pane_chat::structured_sink::make_flatten_sink(
            svc.app_handle.clone(),
            msg_count,
            Some(session_id.clone()),
            session_binding_nonce.clone(),
        );
        let handle = crate::service::pane_chat::spawn_tailer(
            crate::service::pane_chat::TailConfig {
                pane_id: pane_id.clone(),
                provider_id: provider_id.clone(),
                cwd: cwd.clone(),
                session_id: session_id.clone(),
                start_offset: tailer_start_offset,
                poll_interval: crate::service::pane_chat::live_chat_poll_interval(),
            },
            svc.agent_registry.clone(),
            sink,
        );
        svc.chat_registry
            .insert(pane_id.clone(), session_id.clone(), handle);
    }
    let tailer_ms = tailer_started.elapsed().as_millis();
    let total_ms = started.elapsed().as_millis();
    if total_ms > 250 {
        log::warn!(
            "[chat_start.slow] pane={} provider={} total_ms={} mux={} detect={} path={} read={} tailer={} total_bytes={} messages={}",
            pane_id,
            provider_id,
            total_ms,
            mux_ms,
            detect_ms,
            path_ms,
            read_ms,
            tailer_ms,
            total,
            msg_count
        );
    }

    Ok(ChatStartResult {
        text: String::new(),
        start_offset: 0,
        offset: total,
        pending: false,
        messages: Some(prepend_pending_greeting(svc, &pane_id, messages)),
        cursor: None,
        source_session_id: Some(session_id),
        session_binding_nonce,
    })
}

fn jsonl_tail_window_start(path: &std::path::Path, total: u64, window: u64) -> u64 {
    let rough = total.saturating_sub(window);
    if rough == 0 {
        return 0;
    }
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return rough;
    };
    if file.seek(SeekFrom::Start(rough)).is_err() {
        return rough;
    }
    let mut buf = [0u8; 8192];
    let Ok(n) = file.read(&mut buf) else {
        return rough;
    };
    buf[..n]
        .iter()
        .position(|b| *b == b'\n')
        .map(|idx| rough + idx as u64 + 1)
        .unwrap_or(rough)
}

fn jsonl_snapshot_from_offset(
    registry: &std::sync::Arc<crate::agent::registry::AgentRegistry>,
    provider_id: &str,
    path: &std::path::Path,
    offset: u64,
) -> Vec<crate::agent::chat_types::ChatMsg> {
    registry
        .with_provider(provider_id, |p| {
            let mut delta = p.parse_session_delta(path, offset);
            for m in &mut delta.messages {
                crate::service::pane_chat::enrich_interactive(m, p);
            }
            delta
                .messages
                .iter()
                .enumerate()
                .flat_map(|(i, r)| crate::service::pane_chat::rich_to_chat_msgs(r, i))
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub fn session_transcript(
    svc: tauri::State<'_, ServiceState>,
    provider: String,
    cwd: String,
    session_id: String,
) -> Result<ChatStartResult, String> {
    let (messages, _total) =
        crate::service::pane_chat::session_chat_snapshot(&svc, &provider, &cwd, &session_id, 0)?;
    Ok(ChatStartResult {
        text: String::new(),
        start_offset: 0,
        offset: 0,
        pending: false,
        messages: Some(messages),
        cursor: None,
        source_session_id: Some(session_id),
        session_binding_nonce: None,
    })
}

#[tauri::command]
pub async fn chat_stop(svc: tauri::State<'_, ServiceState>, pane_id: String) -> Result<(), String> {
    svc.chat_registry.stop(&pane_id);
    Ok(())
}

/// Format an uploaded voice attachment for sending to an agent: transcribe it
/// (via the configured transcriber daemon) and wrap it in the same
/// `-=-codeterm:voice ...-=-` marker shape the Telegram bridge uses.
#[tauri::command]
pub async fn transcribe_voice_attachment(
    svc: tauri::State<'_, ServiceState>,
    path: String,
) -> Result<String, String> {
    let active = crate::config::load_config()
        .integrations
        .voice_transcribe
        .active;
    Ok(crate::bridge::voice_transcribe::format_attachment_line(
        &path,
        "voice",
        None,
        &svc.transcribe_registry,
        &active,
    )
    .await)
}

#[derive(serde::Serialize)]
pub struct ChatBackfillResult {
    pub text: String,
    pub start_offset: u64,
    pub end_offset: u64,
}

#[tauri::command]
pub async fn chat_backfill(
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
    before_offset: u64,
    max_bytes: Option<u64>,
) -> Result<ChatBackfillResult, String> {
    chat_backfill_core(&svc, pane_id, before_offset, max_bytes)
}

/// Shared by the Tauri command and the web mirror's
/// POST /api/panes/{id}/chat-raw/backfill.
pub fn chat_backfill_core(
    svc: &ServiceState,
    pane_id: String,
    before_offset: u64,
    max_bytes: Option<u64>,
) -> Result<ChatBackfillResult, String> {
    let (provider_id, cwd, session_id) = {
        let m = svc.mux.write();
        let pane = m.get_pane(&pane_id).ok_or("pane not found")?;
        (
            pane.agent_type.clone().ok_or("no agent_type")?,
            pane.cwd.clone().unwrap_or_default(),
            pane.agent_session_id.clone().ok_or("no session_id")?,
        )
    };
    let path = svc
        .agent_registry
        .with_provider(&provider_id, |p| p.session_jsonl_path(&cwd, &session_id))
        .flatten()
        .ok_or("no JSONL")?;

    let max_bytes = max_bytes.unwrap_or(128 * 1024);
    let start = before_offset.saturating_sub(max_bytes);

    let bytes = std::fs::read(&path).map_err(|e| format!("read jsonl: {e}"))?;
    let total = bytes.len() as u64;
    let clamped_end = before_offset.min(total) as usize;
    let clamped_start = (start as usize).min(clamped_end);
    let window = &bytes[clamped_start..clamped_end];
    // Align to the first \n so the parser doesn't see a partial first line.
    let aligned_offset = if clamped_start == 0 {
        0usize
    } else {
        window
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    };
    let text = String::from_utf8_lossy(&window[aligned_offset..]).to_string();

    Ok(ChatBackfillResult {
        text,
        start_offset: clamped_start as u64 + aligned_offset as u64,
        end_offset: before_offset,
    })
}

#[tauri::command]
pub async fn chat_send_text(
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
    text: String,
) -> Result<String, String> {
    // Agent without knowing its pane id — parity with the REST send route.
    let pane_id = svc.mux.read().resolve_pane_id(&pane_id).unwrap_or(pane_id);
    let run_stages =
        crate::chatpipeline::is_user_turn_send(false, false, svc.pane_has_agent(&pane_id));
    let text = if run_stages {
        svc.run_outbound_stages(&pane_id, text)
    } else {
        text
    };
    // A chat send always submits.
    svc.send_text_when_ready(&pane_id, &text, true).await?;
    Ok(pane_id)
}

/// P1: open a non-PTY chatBackend pane driven by `plugin_id` (a plugin declaring
/// the `chatBackend` capability). The frontend Extensions "Open chat" action
/// calls this; `send`/`get-text` on the returned pane route to the plugin via
/// the PaneTransport seam. Returns the new pane id.
#[tauri::command]
pub async fn open_chat_backend_pane(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
    workspace_id: Option<String>,
    config: Option<serde_json::Value>,
    system_prompt: Option<String>,
    preset: Option<String>,
    model: Option<String>,
) -> Result<String, String> {
    use crate::service::spawn::{PaneCommand, PaneVisibility, SpawnPaneSpec};
    let mut config = config.unwrap_or(serde_json::Value::Null);
    let overrides = crate::service::spawn::chatbackend_spawn_config(
        system_prompt.as_deref(),
        preset.as_deref(),
        model.as_deref(),
        None,
        None,
        None,
    );
    if let (Some(base), Some(extra)) = (config.as_object_mut(), overrides.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    } else if !overrides.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        config = overrides;
    }
    crate::service::spawn::reject_direct_watcher_chatbackend_config(&config)?;
    let handle = svc.spawn_pane(SpawnPaneSpec {
        workspace_id,
        visibility: PaneVisibility::Visible,
        command: PaneCommand::ChatBackend {
            plugin_id,
            config,
            role: None,
            role_profile: None,
            can_spawn: None,
            watcher: None,
        },
        ..Default::default()
    })?;
    Ok(handle.pane_id)
}

/// P1: current live state of an open chatBackend session — the model it is
/// talking to — so the tab/bubble can show it. Core asks the capability
/// resolved by `plugin_id`; never a hardcoded id.
#[tauri::command]
pub async fn chatbackend_session_info(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
    session_id: String,
) -> Result<crate::chatbackend::ChatSessionInfo, String> {
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || svc.chatbackend_session_info(&plugin_id, &session_id))
        .await
        .map_err(|e| e.to_string())?
}

/// P1: switch the model an open chatBackend shell session uses on its next turn,
/// returning the refreshed session info. Backends that cannot switch surface an
/// error (the capability's `setModel` is optional).
#[tauri::command]
pub async fn chatbackend_switch_model(
    svc: tauri::State<'_, ServiceState>,
    plugin_id: String,
    session_id: String,
    model: String,
) -> Result<crate::chatbackend::ChatSessionInfo, String> {
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || svc.chatbackend_set_model(&plugin_id, &session_id, &model))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn workspace_plugins_root() -> std::path::PathBuf {
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")).join("plugins")
    }

    fn claude_registry() -> Arc<crate::agent::registry::AgentRegistry> {
        let plugin_js = std::fs::read_to_string(
            workspace_plugins_root()
                .join("provider-claude")
                .join("plugin.js"),
        )
        .expect("read claude plugin.js");
        let manifest_json = r#"{
            "id": "claude",
            "version": "0.1.0",
            "displayName": "Claude Code",
            "spawn": {
                "readinessStrategy": {"kind": "fixed_delay", "durationMs": 0},
                "taskDelivery": "pty_input",
                "verifySubmit": {"kind": "none"}
            },
            "sessions": {
                "rootDir": "~/.claude/projects",
                "cwdEncoding": "dash_encoded",
                "fileExtension": "jsonl"
            }
        }"#;
        let manifest: crate::plugin::js_runtime::PluginManifest =
            serde_json::from_str(manifest_json).expect("parse claude manifest");
        let vm = crate::plugin::js_runtime::PluginVm::from_source_with(
            "claude",
            plugin_js,
            Some(manifest_json.to_string()),
        )
        .expect("init claude VM");
        let provider = crate::plugin::js_runtime::JsProvider::new(manifest, Arc::new(vm));
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(provider));
        Arc::new(registry)
    }

    #[test]
    fn large_jsonl_tail_window_returns_normalized_history() {
        if !workspace_plugins_root().join("provider-claude").exists() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dante.jsonl");
        let compact = serde_json::json!({
            "type": "user",
            "uuid": "compact-user",
            "isVisibleInTranscriptOnly": true,
            "isCompactSummary": true,
            "message": {
                "role": "user",
                "content": "This session is being continued from a previous conversation.\n\nSummary:\n- Dante context"
            }
        });
        let task = serde_json::json!({
            "type": "user",
            "uuid": "task-user",
            "origin": {"kind": "task-notification"},
            "message": {
                "role": "user",
                "content": "<task-notification>\n<status>completed</status>\n</task-notification>"
            }
        });
        let normal = serde_json::json!({
            "type": "user",
            "uuid": "normal-user",
            "message": {"role": "user", "content": "real user message"}
        });
        std::fs::write(&path, format!("{}\n{}\n{}\n", compact, task, normal)).unwrap();

        let total = std::fs::metadata(&path).unwrap().len();
        let start = jsonl_tail_window_start(&path, total, 1024 * 1024);
        let msgs = jsonl_snapshot_from_offset(&claude_registry(), "claude", &path, start);

        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].kind, "system");
        assert_eq!(msgs[0].system_subtype.as_deref(), Some("compact_summary"));
        assert!(msgs[0].collapsed);
        assert_eq!(msgs[1].kind, "status");
        assert_eq!(msgs[1].content, "Task completed");
        assert_eq!(msgs[1].system_subtype.as_deref(), Some("task_notification"));
        assert_eq!(msgs[2].kind, "user");
        assert_eq!(msgs[2].content, "real user message");
    }
}
