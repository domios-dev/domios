use crate::agent::git_info::{self, GitInfo};
use crate::multiplexer::commands::MuxState;
use crate::service::ServiceState;
use crate::ScreenStateHandle;

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
    Ok(tokio::task::spawn_blocking(move || git_info::get_git_info(&cwd))
        .await
        .map_err(|e| format!("join: {}", e))?)
}

#[tauri::command]
pub async fn list_agent_providers(
    svc: tauri::State<'_, ServiceState>,
) -> Result<serde_json::Value, String> {
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::ListAgentProviders,
        svc.as_ref(),
    )?;
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
        let pane = m.get_pane(&pane_id)
            .ok_or_else(|| "Pane not found".to_string())?;
        let sid = pane.agent_session_id.clone();
        let cwd = pane.cwd.clone()
            .ok_or_else(|| "Pane has no CWD".to_string())?;
        let at = pane.agent_type.clone().unwrap_or_else(crate::config::default_agent_provider);
        (sid, cwd, at)
    };

    let needs_detect = if session_id.is_none() {
        true
    } else if let Some(ref sid) = session_id {
        svc.agent_registry.with_provider(&agent_type, |p| p.session_file_path(&cwd, sid))
            .flatten()
            .map(|path| !path.exists())
            .unwrap_or(true)
    } else { false };

    if needs_detect {
        let exclude = mux.lock().other_session_ids(&pane_id);
        let max_age = Some(std::time::Duration::from_secs(86400));
        let detected = svc.agent_registry.with_provider(&agent_type, |p| {
            p.detect_session_id(&cwd, &exclude, max_age)
        }).flatten();
        if let Some(detected) = detected {
            log::info!("[read_session_jsonl] detected session {} for pane {}", detected, pane_id);
            svc.set_agent_session_id(&pane_id, Some(detected.clone()));
            svc.emit_state_changed();
            session_id = Some(detected);
        }
    }

    let session_id = session_id
        .ok_or_else(|| "No agent session linked to this pane".to_string())?;

    let jsonl_path = svc.agent_registry.with_provider(&agent_type, |p| p.session_file_path(&cwd, &session_id))
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
        file.seek(SeekFrom::Start(skip)).map_err(|e| e.to_string())?;
        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    } else if let Some(tail) = tail_bytes {
        let start = if file_len > tail { file_len - tail } else { 0 };
        file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
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
