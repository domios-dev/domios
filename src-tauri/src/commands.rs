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
pub fn get_default_cwd() -> Result<String, String> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

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
                    if !agent_events.is_empty() {
                        log::debug!("[agent] Events detected: {:?} for pty {}", agent_events, &pty_id[..8]);
                    }

                    // Cooldown: skip if same event type fired within last 10s
                    use std::sync::Mutex as StdMutex;
                    use std::collections::HashMap;
                    static LAST_NOTIF: std::sync::LazyLock<StdMutex<HashMap<(String, String), std::time::Instant>>> =
                        std::sync::LazyLock::new(|| StdMutex::new(HashMap::new()));

                    for event in &agent_events {
                        let event_key = (pty_id.clone(), format!("{:?}", event));
                        {
                            let mut map = LAST_NOTIF.lock().unwrap();
                            let now = std::time::Instant::now();
                            if let Some(last) = map.get(&event_key) {
                                if now.duration_since(*last).as_secs() < 10 {
                                    continue;
                                }
                            }
                            map.insert(event_key, now);
                        }
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
    /// Current revision from bundled skill
    pub revision: u32,
    /// Installed revision (0 if not installed)
    pub installed_revision: u32,
    /// True if installed version is outdated
    pub update_available: bool,
}

/// Extract revision number from file content.
/// Looks for `<!-- revision: N -->` pattern.
fn extract_revision(content: &str) -> u32 {
    content.lines()
        .find_map(|l| {
            let trimmed = l.trim();
            if trimmed.starts_with("<!-- revision:") && trimmed.ends_with("-->") {
                let inner = trimmed
                    .trim_start_matches("<!-- revision:")
                    .trim_end_matches("-->")
                    .trim();
                inner.parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
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

        let revision = extract_revision(&content);
        let installed_path = installed_dir.join(&id);
        let installed = installed_path.exists();
        let installed_revision = if installed {
            let installed_skill = installed_path.join("SKILL.md");
            std::fs::read_to_string(&installed_skill)
                .map(|c| extract_revision(&c))
                .unwrap_or(0)
        } else {
            0
        };
        let update_available = installed && installed_revision < revision;
        skills.push(SkillInfo { id, name, description, installed, revision, installed_revision, update_available });
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

<!-- revision: 3 -->
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

/// Check if codeterm rule is installed and up to date.
/// Returns: { installed: bool, update_available: bool, revision: u32, installed_revision: u32 }
#[tauri::command]
pub fn check_rule_installed() -> Result<RuleStatus, String> {
    let path = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".claude")
        .join("rules")
        .join("codeterm.md");
    let bundled_revision = extract_revision(CODETERM_RULE);
    if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let installed_revision = extract_revision(&content);
        Ok(RuleStatus {
            installed: true,
            update_available: installed_revision < bundled_revision,
            revision: bundled_revision,
            installed_revision,
        })
    } else {
        Ok(RuleStatus {
            installed: false,
            update_available: false,
            revision: bundled_revision,
            installed_revision: 0,
        })
    }
}

#[derive(serde::Serialize)]
pub struct RuleStatus {
    pub installed: bool,
    pub update_available: bool,
    pub revision: u32,
    pub installed_revision: u32,
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

// ---------------------------------------------------------------------------
// Claude Code hooks management
// ---------------------------------------------------------------------------

fn claude_settings_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

const CODETERM_HOOK_MARKER: &str = "codeterm-hook";

/// Check if Claude Code hooks are installed for CodeTerm notifications.
#[tauri::command]
pub fn check_claude_hooks() -> Result<ClaudeHooksStatus, String> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok(ClaudeHooksStatus {
            settings_exists: false,
            hooks_installed: false,
            notification_hook: false,
            stop_hook: false,
        });
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings.json: {}", e))?;

    let has_marker = content.contains(CODETERM_HOOK_MARKER);
    let notification_hook = json.pointer("/hooks/Notification")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|h| {
            h.get("hooks").and_then(|hs| hs.as_array())
                .map(|hs| hs.iter().any(|h| h.get("command").and_then(|c| c.as_str()).unwrap_or("").contains("codeterm")))
                .unwrap_or(false)
        }))
        .unwrap_or(false);
    let stop_hook = json.pointer("/hooks/Stop")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|h| {
            h.get("hooks").and_then(|hs| hs.as_array())
                .map(|hs| hs.iter().any(|h| h.get("command").and_then(|c| c.as_str()).unwrap_or("").contains("codeterm")))
                .unwrap_or(false)
        }))
        .unwrap_or(false);

    Ok(ClaudeHooksStatus {
        settings_exists: true,
        hooks_installed: has_marker || (notification_hook && stop_hook),
        notification_hook,
        stop_hook,
    })
}

#[derive(serde::Serialize)]
pub struct ClaudeHooksStatus {
    pub settings_exists: bool,
    pub hooks_installed: bool,
    pub notification_hook: bool,
    pub stop_hook: bool,
}

/// Install Claude Code hooks that notify CodeTerm on permission requests and task completion.
#[tauri::command]
pub fn install_claude_hooks(api_port: Option<u16>) -> Result<(), String> {
    let path = claude_settings_path()?;
    let port = api_port.unwrap_or(7685);

    // Ensure ~/.claude/ directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    // Read existing settings or create empty
    let mut json: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))?
    } else {
        serde_json::json!({})
    };

    // Build hook commands
    // Single quotes around JSON body prevent shell from interpreting the content.
    // The \" inside are JSON escapes in the settings.json file — they become " at runtime.
    let notify_cmd = format!(
        r#"curl -s -X POST http://localhost:{port}/api/notifications -H 'Content-Type: application/json' -d '{{"title":"Claude Code","body":"Needs permission","level":"warning"}}' > /dev/null 2>&1 || true"#
    );
    let stop_cmd = format!(
        r#"curl -s -X POST http://localhost:{port}/api/notifications -H 'Content-Type: application/json' -d '{{"title":"Claude Code","body":"Task complete","level":"info"}}' > /dev/null 2>&1 || true"#
    );

    // Ensure hooks object exists
    if json.get("hooks").is_none() {
        json["hooks"] = serde_json::json!({});
    }

    // Notification hook (permission_prompt)
    let notif_hook = serde_json::json!({
        "matcher": "permission_prompt",
        "hooks": [{
            "type": "command",
            "command": notify_cmd,
            "timeout": 5000,
            "_source": CODETERM_HOOK_MARKER
        }]
    });

    // Stop hook (task complete)
    let stop_hook = serde_json::json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": stop_cmd,
            "timeout": 5000,
            "_source": CODETERM_HOOK_MARKER
        }]
    });

    // Add to existing hooks arrays (don't overwrite user's hooks)
    let hooks = json["hooks"].as_object_mut()
        .ok_or("hooks is not an object")?;

    // Remove old codeterm hooks first
    for key in &["Notification", "Stop"] {
        if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
            arr.retain(|h| {
                !h.get("hooks").and_then(|hs| hs.as_array())
                    .map(|hs| hs.iter().any(|h| h.get("_source").and_then(|s| s.as_str()) == Some(CODETERM_HOOK_MARKER)))
                    .unwrap_or(false)
            });
        }
    }

    // Add new hooks
    if !hooks.contains_key("Notification") {
        hooks.insert("Notification".into(), serde_json::json!([]));
    }
    hooks["Notification"].as_array_mut().unwrap().push(notif_hook);

    if !hooks.contains_key("Stop") {
        hooks.insert("Stop".into(), serde_json::json!([]));
    }
    hooks["Stop"].as_array_mut().unwrap().push(stop_hook);

    // Write back
    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    log::info!("Claude Code hooks installed at {:?}", path);
    Ok(())
}

/// Remove CodeTerm hooks from Claude Code settings.
#[tauri::command]
pub fn uninstall_claude_hooks() -> Result<(), String> {
    let path = claude_settings_path()?;
    if !path.exists() { return Ok(()); }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings.json: {}", e))?;

    if let Some(hooks) = json.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        for key in &["Notification", "Stop"] {
            if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
                arr.retain(|h| {
                    !h.get("hooks").and_then(|hs| hs.as_array())
                        .map(|hs| hs.iter().any(|h| h.get("_source").and_then(|s| s.as_str()) == Some(CODETERM_HOOK_MARKER)))
                        .unwrap_or(false)
                });
            }
        }
    }

    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    log::info!("Claude Code hooks removed");
    Ok(())
}

/// Save clipboard image bytes to a temp file, return the path.
/// Old images are cleaned up (keep last 20).
#[tauri::command]
pub async fn save_clipboard_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    let temp_dir = std::env::temp_dir().join("codeterm-images");
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
    // Cleanup old files (keep last 20)
    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        let mut files: Vec<_> = entries.flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .collect();
        if files.len() > 20 {
            files.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH));
            for old in &files[..files.len() - 20] {
                let _ = std::fs::remove_file(old.path());
            }
        }
    }
    let filename = format!("clipboard-{}.{}", uuid::Uuid::new_v4(), ext);
    let path = temp_dir.join(&filename);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Copy image file to Windows clipboard as PNG so Claude Code can read it via Ctrl+V.
#[tauri::command]
pub async fn copy_image_to_clipboard(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // Use PowerShell to copy image as both Image and PNG stream to clipboard
        let ps_script = format!(
            r#"
            Add-Type -AssemblyName System.Windows.Forms
            Add-Type -AssemblyName System.Drawing
            $img = [System.Drawing.Image]::FromFile('{}')
            $data = New-Object System.Windows.Forms.DataObject
            $data.SetImage($img)
            $ms = New-Object System.IO.MemoryStream
            $img.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
            $ms.Position = 0
            $data.SetData('PNG', $false, $ms)
            [System.Windows.Forms.Clipboard]::SetDataObject($data, $true)
            $img.Dispose()
            "#,
            path.replace("'", "''").replace('\\', "\\\\")
        );
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &ps_script])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Clipboard copy failed: {}", stderr));
        }
    }
    Ok(())
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
