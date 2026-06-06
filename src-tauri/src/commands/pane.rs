use crate::agent::git_info::{self, GitInfo};
use crate::multiplexer::commands::MuxState;
use crate::service::ServiceState;
use crate::ScreenStateHandle;

// NOTE: `crate::agent::chat_types::{RichMessage, SessionDelta}` and the
// parallel `spawn_tailer` / `DeltaSink` path are reserved for future
// non-UI consumers (mem worker, mesh log shipping, etc.). The chat UI
// uses the raw-text tailer below — the TS plugin owns parsing.

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
    let result = crate::dispatch::dispatch(&cmd, svc.as_ref())?;
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
    let result = crate::dispatch::dispatch(&cmd, svc.as_ref())?;
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
    let result = crate::dispatch::dispatch(&cmd, svc.as_ref())?;
    Ok(result
        .into_json()
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string())
}

#[tauri::command]
pub async fn get_pane_git_info(
    mux: tauri::State<'_, MuxState>,
    pane_id: String,
) -> Result<Option<GitInfo>, String> {
    let cwd = {
        let mux = mux.lock();
        mux.get_pane(&pane_id).and_then(|p| p.cwd.clone())
    };
    let Some(cwd) = cwd else { return Ok(None) };
    Ok(
        tokio::task::spawn_blocking(move || git_info::get_git_info(&cwd))
            .await
            .map_err(|e| format!("join: {}", e))?,
    )
}

#[tauri::command]
pub async fn list_agent_providers(
    svc: tauri::State<'_, ServiceState>,
) -> Result<serde_json::Value, String> {
    let result =
        crate::dispatch::dispatch(&crate::dispatch::Command::ListAgentProviders, svc.as_ref())?;
    Ok(result.into_json())
}

#[tauri::command]
pub async fn wait_shell_ready(
    screen_state: tauri::State<'_, ScreenStateHandle>,
    mux: tauri::State<'_, MuxState>,
    pane_id: String,
    timeout_ms: Option<u64>,
) -> Result<bool, String> {
    let pty_id = {
        let m = mux.lock();
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
    // 1. Get agent_session_id, agent_type, and cwd from pane
    let (mut session_id, cwd, agent_type) = {
        let m = mux.lock();
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

    let needs_detect = if session_id.is_none() {
        true
    } else if let Some(ref sid) = session_id {
        svc.agent_registry
            .with_provider(&agent_type, |p| p.session_file_path(&cwd, sid))
            .flatten()
            .map(|path| !path.exists())
            .unwrap_or(true)
    } else {
        false
    };

    if needs_detect {
        let exclude = mux.lock().other_session_ids(&pane_id);
        let max_age = Some(std::time::Duration::from_secs(86400));
        let detected = svc
            .agent_registry
            .with_provider(&agent_type, |p| {
                p.detect_session_id(&cwd, &exclude, max_age)
            })
            .flatten();
        if let Some(detected) = detected {
            log::info!(
                "[read_session_jsonl] detected session {} for pane {}",
                detected,
                pane_id
            );
            svc.set_agent_session_id(&pane_id, Some(detected.clone()));
            svc.emit_state_changed();
            session_id = Some(detected);
        }
    }

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
        let start = if file_len > tail { file_len - tail } else { 0 };
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

// ---------------------------------------------------------------------------
// Chat UI mode — thin Tauri wrappers over the pane_chat service.
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct ChatStartResult {
    /// Raw JSONL text from the recent window — frontend parses via the
    /// claude TS plugin's `parseJsonlSession`. The window is anchored to
    /// a `\n` boundary so the parser never sees a partial first line.
    pub text: String,
    pub start_offset: u64,
    pub offset: u64,
}

#[tauri::command]
pub async fn chat_start(
    svc: tauri::State<'_, ServiceState>,
    pane_id: String,
) -> Result<ChatStartResult, String> {
    chat_start_core(&svc, pane_id)
}

/// Shared by the Tauri command and the web mirror's
/// POST /api/panes/{id}/chat-raw/start — identical result shape, so the
/// chat view renders the same in both shells.
pub fn chat_start_core(svc: &ServiceState, pane_id: String) -> Result<ChatStartResult, String> {
    let (provider_id, cwd, session_id) = {
        let m = svc.mux.lock();
        let pane = m.get_pane(&pane_id).ok_or("pane not found")?;
        let provider = pane.agent_type.clone().ok_or("pane has no agent_type")?;
        let cwd = pane.cwd.clone().unwrap_or_default();
        let sid = pane
            .agent_session_id
            .clone()
            .ok_or("pane has no session_id yet")?;
        (provider, cwd, sid)
    };

    // Chat view is a reader for the tab's saved conversation, not a session
    // detector. Recency/cwd-based detection is unsafe here: a dev console or
    // sibling pane can create a newer JSONL in the same project and would make
    // this tab render the wrong chat.
    let path = svc
        .agent_registry
        .with_provider(&provider_id, |p| p.session_jsonl_path(&cwd, &session_id))
        .flatten()
        .ok_or("provider has no JSONL")?;
    if !path.exists() {
        return Err(format!(
            "JSONL not found for pane session {}: {}",
            session_id,
            path.display()
        ));
    }

    // Read the tail (~512KB) so toggle reveals plenty of context. The TS
    // parser is fast even at 1MB — no need for a smaller window.
    let snapshot_window: u64 = 512 * 1024;
    let bytes = std::fs::read(&path).map_err(|e| format!("read jsonl: {e}"))?;
    let total = bytes.len() as u64;
    let start = total.saturating_sub(snapshot_window) as usize;
    // Align start to a line boundary — handing the parser a partial
    // first line forces it to discard data that would otherwise render.
    let aligned_start = if start == 0 {
        0
    } else {
        bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| start + i + 1)
            .unwrap_or(start)
    };
    let text = String::from_utf8_lossy(&bytes[aligned_start..]).to_string();

    // Multiple webviews can attach to the same pane. The tailer is per pane,
    // but subscriptions are per client; share the existing worker when one is
    // already running so one client's stop() cannot kill another's stream.
    if !svc.chat_registry.acquire(&pane_id) {
        let sink = crate::service::pane_chat::raw_text_sink::make_sink(svc.app_handle.clone());
        let handle = crate::service::pane_chat::spawn_raw_tailer(
            crate::service::pane_chat::RawTailConfig {
                pane_id: pane_id.clone(),
                path: path.clone(),
                start_offset: total,
                poll_interval: std::time::Duration::from_millis(500),
            },
            sink,
        );
        svc.chat_registry.insert(pane_id, handle);
    }

    Ok(ChatStartResult {
        text,
        start_offset: aligned_start as u64,
        offset: total,
    })
}

#[tauri::command]
pub async fn chat_stop(svc: tauri::State<'_, ServiceState>, pane_id: String) -> Result<(), String> {
    svc.chat_registry.stop(&pane_id);
    Ok(())
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
        let m = svc.mux.lock();
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
    enter: Option<bool>,
) -> Result<(), String> {
    let enter = enter.unwrap_or(true);
    svc.send_text_when_ready(&pane_id, &text, enter).await
}
