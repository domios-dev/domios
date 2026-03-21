use std::collections::HashMap;
use std::io::Read;
use tauri::AppHandle;

use crate::agent::detector;
use crate::agent::git_info::{self, GitInfo};
use crate::multiplexer::commands::MuxState;
use crate::notification::NotifState;
use crate::api::events::EventBroadcasterState;
use crate::{AgentTrackerState, OutputBufferState, PtyState};

/// Spawn a new PTY. Returns the PTY ID.
/// NO background thread — frontend polls via read_pty command.
#[tauri::command]
pub async fn spawn_pty(
    state: tauri::State<'_, PtyState>,
    _app: AppHandle,
    command: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    predicted_id: Option<String>,
) -> Result<String, String> {
    let env_vars: HashMap<String, String> = HashMap::new();

    let (pty_id, _session) = {
        let mut manager = state.lock();
        manager.spawn_with_id(predicted_id, command, cwd, cols, rows, env_vars)?
    };

    log::info!("PTY {} spawned", pty_id);
    Ok(pty_id)
}

/// Read available data from PTY. Called by frontend in requestAnimationFrame loop.
/// Uses spawn_blocking to avoid blocking the async thread pool.
#[tauri::command]
pub async fn read_pty(
    state: tauri::State<'_, PtyState>,
    output_buf: tauri::State<'_, OutputBufferState>,
    mux: tauri::State<'_, MuxState>,
    tracker: tauri::State<'_, AgentTrackerState>,
    events: tauri::State<'_, EventBroadcasterState>,
    app: AppHandle,
    pty_id: String,
) -> Result<Vec<u8>, String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    let ob = (*output_buf).clone();
    let pid = pty_id.clone();

    // Take reader from session — only one readLoop can own it at a time.
    // If reader was already taken (by a previous readLoop that's blocking in read()),
    // create a new one via try_clone_reader.
    let mut reader_to_use = {
        let mut reader_slot = session.reader.lock().unwrap();
        match reader_slot.take() {
            Some(r) => r,
            None => {
                // Reader taken by old readLoop — clone a new one from master
                let pair = session.pair.lock().unwrap();
                pair.master.try_clone_reader().map_err(|e| e.to_string())?
            }
        }
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut buf = vec![0u8; 8192];
        let n = reader_to_use.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("EOF".to_string());
        }
        buf.truncate(n);
        ob.lock().append(&pid, &buf);

        // Return reader so next call can reuse it
        Ok((buf, reader_to_use))
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok((data, reader_back)) => {
            // Put reader back for next call
            let mut reader_slot = session.reader.lock().unwrap();
            *reader_slot = Some(reader_back);

            // Status tracking + agent event detection
            {
                let mut t = tracker.lock();
                let prev = t.get_status(&pty_id);
                t.update_output(&pty_id, &data);

                // Detect Claude Code events from output text (strip ANSI first)
                if let Ok(raw) = std::str::from_utf8(&data) {
                    let text = crate::api::routes::strip_ansi_codes(raw);
                    let agent_events = detector::detect_claude_events(&text);
                    for event in &agent_events {
                        match event {
                            detector::AgentEvent::PermissionRequest => {
                                t.set_waiting(&pty_id);
                                let info = mux.lock().snapshot().panes.iter()
                                    .find(|(_, p)| p.pty_id.as_deref() == Some(&pty_id))
                                    .map(|(id, p)| (id.clone(), p.title.clone()));
                                if let Some((pid, title)) = info {
                                    let notif_title = format!("{} needs approval", title);
                                    use tauri::Emitter;
                                    let _ = app.emit("notification-created", serde_json::json!({
                                        "id": uuid::Uuid::new_v4().to_string(),
                                        "title": &notif_title,
                                        "body": "Agent is waiting for your permission",
                                        "level": "Warning",
                                        "pane_id": &pid,
                                        "workspace_id": serde_json::Value::Null,
                                    }));
                                    crate::api::routes::send_os_notification_if_unfocused(
                                        &app, &notif_title, Some("Agent is waiting for your permission"),
                                    );
                                    // Broadcast to webhooks + websockets
                                    events.broadcast("agent.permission_request", serde_json::json!({
                                        "pane_id": &pid, "title": &title,
                                    }));
                                }
                            }
                            detector::AgentEvent::TaskComplete => {
                                if prev == crate::agent::status::PaneStatus::Working {
                                    let info = mux.lock().snapshot().panes.iter()
                                        .find(|(_, p)| p.pty_id.as_deref() == Some(&pty_id))
                                        .map(|(id, p)| (id.clone(), p.title.clone()));
                                    if let Some((pid, title)) = info {
                                        let notif_title = format!("{} finished", title);
                                        use tauri::Emitter;
                                        let _ = app.emit("notification-created", serde_json::json!({
                                            "id": uuid::Uuid::new_v4().to_string(),
                                            "title": &notif_title,
                                            "body": "Task complete",
                                            "level": "Info",
                                            "pane_id": &pid,
                                            "workspace_id": serde_json::Value::Null,
                                        }));
                                        crate::api::routes::send_os_notification_if_unfocused(
                                            &app, &notif_title, Some("Task complete"),
                                        );
                                        events.broadcast("agent.task_complete", serde_json::json!({
                                            "pane_id": &pid, "title": &title,
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }

                let now = t.get_status(&pty_id);
                if now != prev {
                    let pane_id = mux.lock().snapshot().panes.iter()
                        .find(|(_, p)| p.pty_id.as_deref() == Some(&pty_id))
                        .map(|(id, _)| id.clone());
                    if let Some(pid) = pane_id {
                        use tauri::Emitter;
                        let _ = app.emit("pane-status-changed", serde_json::json!({
                            "pane_id": pid,
                            "status": format!("{:?}", now),
                        }));
                    }
                }
            }

            Ok(data)
        }
        Err(e) => {
            // PTY died
            tracker.lock().set_dead(&pty_id);
            let pane_id = {
                let m = mux.lock();
                let snap = m.snapshot();
                snap.panes.iter()
                    .find(|(_, p)| p.pty_id.as_deref() == Some(&pty_id))
                    .map(|(id, _)| id.clone())
            };
            if let Some(pid) = pane_id {
                use tauri::Emitter;
                let _ = app.emit("pane-status-changed", serde_json::json!({
                    "pane_id": pid,
                    "status": "Dead",
                }));
            }
            Err(e)
        },
    }
}

/// Write string to PTY.
#[tauri::command]
pub async fn write_pty_string(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
    data: String,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    let mut writer = session.writer.lock().unwrap();
    use std::io::Write;
    writer.write_all(data.as_bytes()).map_err(|e| format!("Write failed: {}", e))?;
    writer.flush().map_err(|e| format!("Flush failed: {}", e))?;
    Ok(())
}

/// Write raw bytes to PTY.
#[tauri::command]
pub async fn write_pty(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    let mut writer = session.writer.lock().unwrap();
    use std::io::Write;
    writer.write_all(&data).map_err(|e| format!("Write failed: {}", e))?;
    writer.flush().map_err(|e| format!("Flush failed: {}", e))?;
    Ok(())
}

/// Resize PTY.
#[tauri::command]
pub async fn resize_pty(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    let pair = session.pair.lock().unwrap();
    pair.master.resize(portable_pty::PtySize {
        rows, cols, pixel_width: 0, pixel_height: 0,
    }).map_err(|e| format!("Resize failed: {}", e))?;
    Ok(())
}

/// Kill PTY.
#[tauri::command]
pub async fn kill_pty(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
) -> Result<(), String> {
    let mut manager = state.lock();
    manager.remove(&pty_id);
    Ok(())
}

/// Get captured PTY output.
#[tauri::command]
pub async fn get_pane_output(
    output_buf: tauri::State<'_, OutputBufferState>,
    mux: tauri::State<'_, MuxState>,
    pane_id: String,
) -> Result<String, String> {
    let pty_id = {
        let m = mux.lock();
        m.get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
            .ok_or_else(|| "Pane not found or has no PTY".to_string())?
    };
    let buf = output_buf.lock();
    Ok(buf.get_text(&pty_id).unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Skills management
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, Clone)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub installed: bool,
}

/// Resolve skills source directory. Checks (in order):
/// 1. Adjacent to exe (production build)
/// 2. Project root /skills (dev mode — exe in target/debug)
fn find_skills_dir() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        // Production: skills/ next to exe
        let adjacent = exe.parent().unwrap_or(std::path::Path::new(".")).join("skills");
        if adjacent.exists() { return adjacent; }

        // Dev: exe is in src-tauri/target/debug — project root is 3 levels up
        let dev = exe.parent().unwrap_or(std::path::Path::new("."))
            .join("../../../skills");
        if dev.exists() { return dev.canonicalize().unwrap_or(dev); }
    }
    std::path::PathBuf::from("skills")
}

fn claude_skills_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".claude").join("skills"))
        .unwrap_or_default()
}

fn scan_skills() -> Vec<SkillInfo> {
    let dir = find_skills_dir();
    let installed_dir = claude_skills_dir();

    let mut skills = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else { return skills };

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }

        let id = entry.file_name().to_string_lossy().to_string();
        let skill_file = entry.path().join("SKILL.md");
        if !skill_file.exists() { continue; }

        let content = std::fs::read_to_string(&skill_file).unwrap_or_default();
        let name = content.lines()
            .find(|l| l.starts_with("name:"))
            .map(|l| l.trim_start_matches("name:").trim().to_string())
            .unwrap_or_else(|| id.clone());
        let description = content.lines()
            .find(|l| l.starts_with("description:"))
            .map(|l| l.trim_start_matches("description:").trim().to_string())
            .unwrap_or_default();

        let installed = installed_dir.join(&id).exists();
        skills.push(SkillInfo { id, name, description, installed });
    }
    skills
}

/// List available CodeTerm skills and their install status.
#[tauri::command]
pub fn list_skills() -> Result<Vec<SkillInfo>, String> {
    Ok(scan_skills())
}

/// Install a skill by copying it to ~/.claude/skills/
#[tauri::command]
pub fn install_skill(skill_id: String) -> Result<(), String> {
    let source = find_skills_dir().join(&skill_id);
    if !source.exists() {
        return Err(format!("Skill not found: {}", skill_id));
    }

    let dest = claude_skills_dir().join(&skill_id);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    // Copy all files recursively
    fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
        for entry in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if src_path.is_dir() {
                std::fs::create_dir_all(&dst_path).map_err(|e| e.to_string())?;
                copy_dir(&src_path, &dst_path)?;
            } else {
                std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    copy_dir(&source, &dest)
}

/// Uninstall a skill by removing it from ~/.claude/skills/
#[tauri::command]
pub fn uninstall_skill(skill_id: String) -> Result<(), String> {
    let dest = claude_skills_dir().join(&skill_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Agent prompt template — embedded at compile time from docs/AGENT_PROMPT.md
const AGENT_PROMPT: &str = include_str!("../../docs/AGENT_PROMPT.md");

/// Generate setup prompt — embedded template + dynamic skills list.
#[tauri::command]
pub fn get_skills_prompt() -> Result<String, String> {
    let skills = scan_skills();
    let mut prompt = AGENT_PROMPT.to_string();
    if !skills.is_empty() {
        prompt.push_str("\n\nAvailable skills (install to ~/.claude/skills/):\n");
        for s in &skills {
            prompt.push_str(&format!("  - {} — {}\n", s.name, s.description));
        }
    }
    Ok(prompt)
}

/// Embedded CodeTerm rule for Claude Code
const CODETERM_RULE: &str = r#"## CodeTerm Terminal

You are running inside CodeTerm — an AI-first terminal multiplexer. Use the `/codeterm` skill for all terminal operations.

### When to use CodeTerm CLI

- "in terminal" / "в терминале" / "в консоли" → CodeTerm workspace/pane, NOT filesystem
- "create project X" → `codeterm workspace create "X"`
- "split" / "сплитни" → `codeterm pane split`
- "run tests there" → `codeterm send-text PANE_ID "command"`
- Any visual content (diagrams, dashboards) → `codeterm browser open file:///path`
- Notify on completion → `codeterm notify "Done" --level info`

### On session start

Immediately rename your tab to the task summary:
```bash
PANE_ID=$(C:/tools/codeterm.exe pane list --json | jq -r '.[0].id')
C:/tools/codeterm.exe pane rename "$PANE_ID" "short task name"
```

### Do NOT

- Do NOT use `start` or `open` for browser — use `codeterm browser open`
- Do NOT create filesystem directories when user says "create project" — create a workspace
"#;

/// Install CodeTerm rule to ~/.claude/rules/codeterm.md
#[tauri::command]
pub fn install_project_rules() -> Result<(), String> {
    let dest = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".claude")
        .join("rules");
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    std::fs::write(dest.join("codeterm.md"), CODETERM_RULE).map_err(|e| e.to_string())?;
    Ok(())
}

/// Check if codeterm rule is installed in ~/.claude/rules/
#[tauri::command]
pub fn check_rule_installed() -> Result<bool, String> {
    let path = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".claude")
        .join("rules")
        .join("codeterm.md");
    Ok(path.exists())
}

/// Open a folder in the system file explorer.
#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    let resolved = if path.starts_with("~/") || path.starts_with("~\\") {
        dirs::home_dir()
            .ok_or("Cannot find home directory")?
            .join(&path[2..])
    } else {
        std::path::PathBuf::from(&path)
    };
    if resolved.exists() {
        #[cfg(target_os = "windows")]
        { let _ = std::process::Command::new("explorer").arg(&resolved).spawn(); }
        #[cfg(target_os = "macos")]
        { let _ = std::process::Command::new("open").arg(&resolved).spawn(); }
        #[cfg(target_os = "linux")]
        { let _ = std::process::Command::new("xdg-open").arg(&resolved).spawn(); }
    }
    Ok(())
}

/// Save clipboard image bytes to a temp file, return the path.
#[tauri::command]
pub async fn save_clipboard_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    let temp_dir = std::env::temp_dir().join("codeterm-images");
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
    let filename = format!("clipboard-{}.{}", uuid::Uuid::new_v4(), ext);
    let path = temp_dir.join(&filename);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Get network info (local IP, Tailscale IP, API URL).
#[tauri::command]
pub fn get_network_info() -> Result<serde_json::Value, String> {
    let local_ip = crate::api::routes::local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    let tailscale_ip = crate::api::routes::detect_tailscale_ip();
    Ok(serde_json::json!({
        "local_ip": local_ip,
        "tailscale_ip": tailscale_ip,
        "api_port": 7685,
        "api_url": format!("http://{}:7685", local_ip),
        "tailscale_url": tailscale_ip.as_ref().map(|ip| format!("http://{}:7685", ip)),
    }))
}

/// Get git info for pane CWD.
#[tauri::command]
pub fn get_pane_git_info(
    mux: tauri::State<'_, MuxState>,
    pane_id: String,
) -> Result<Option<GitInfo>, String> {
    let mux = mux.lock();
    let pane = mux.get_pane(&pane_id);
    match pane.and_then(|p| p.cwd.as_ref()) {
        Some(cwd) => Ok(git_info::get_git_info(cwd)),
        None => Ok(None),
    }
}
