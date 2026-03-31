use std::collections::HashMap;
use std::io::Read;
use base64::Engine as _;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::agent::git_info::{self, GitInfo};
use crate::multiplexer::commands::MuxState;
use crate::service::ServiceState;
use crate::{AgentTrackerState, OutputBufferState, PtyState, ScreenStateHandle, UpdateStateHandle};

#[tauri::command]
pub async fn check_for_update(
    update_state: tauri::State<'_, UpdateStateHandle>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    let method = update_state.lock().install_method.clone();
    let info = crate::update::checker::check_for_update(&method)
        .await
        .map_err(|e| e.to_string())?;

    if info.update_available {
        use tauri::Emitter;
        let _ = app.emit("update-available", serde_json::to_value(&info).unwrap_or_default());
    }

    let val = serde_json::to_value(&info).map_err(|e| e.to_string())?;
    let mut us = update_state.lock();
    us.last_check = Some(info);
    us.last_check_time = Some(std::time::Instant::now());
    Ok(val)
}

#[tauri::command]
pub async fn install_app_update(app: AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    if let Some(update) = update {
        update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        app.restart();
    } else {
        Err("No update available".into())
    }
}

/// Spawn a new PTY. Returns the PTY ID.
/// NO background thread — frontend polls via read_pty command.
#[tauri::command]
pub fn get_windows_build_number() -> u32 {
    #[cfg(target_os = "windows")]
    {
        // Parse build number from Windows registry or ver command
        use std::process::Command;
        if let Ok(output) = Command::new("cmd").args(["/c", "ver"]).output() {
            let ver = String::from_utf8_lossy(&output.stdout);
            // Format: "Microsoft Windows [Version 10.0.26200.5570]"
            if let Some(start) = ver.rfind('.') {
                // Get the part before last dot: "10.0.XXXXX"
                let before_last = &ver[..start];
                if let Some(build_start) = before_last.rfind('.') {
                    if let Ok(n) = before_last[build_start + 1..].trim().parse::<u32>() {
                        return n;
                    }
                }
            }
        }
        19041 // fallback
    }
    #[cfg(not(target_os = "windows"))]
    { 0 }
}

#[tauri::command]
pub fn get_default_cwd() -> Result<String, String> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn spawn_pty(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    _app: AppHandle,
    command: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    predicted_id: Option<String>,
    pane_id: Option<String>,
) -> Result<String, String> {
    let mut env_vars: HashMap<String, String> = HashMap::new();
    if let Some(ref pid) = pane_id {
        env_vars.insert("CODETERM_PANE_ID".to_string(), pid.clone());
        // Pass role/parent info so agents know their place in hierarchy
        let mux = mux_state.lock();
        if let Some(pane) = mux.get_pane(pid) {
            if let Some(ref role) = pane.role {
                env_vars.insert("CODETERM_ROLE".to_string(), format!("{:?}", role));
            }
            if let Some(ref parent) = pane.parent_pane_id {
                env_vars.insert("CODETERM_PARENT_PANE_ID".to_string(), parent.clone());
            }
        }
        drop(mux);
    }
    env_vars.insert(
        "CODETERM_SOCKET_PATH".to_string(),
        crate::socket::server::socket_display_path(),
    );

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
    screen_state: tauri::State<'_, ScreenStateHandle>,
    mux: tauri::State<'_, MuxState>,
    tracker: tauri::State<'_, AgentTrackerState>,
    svc: tauri::State<'_, ServiceState>,
    app: AppHandle,
    pty_id: String,
) -> Result<Vec<u8>, String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    let ob = (*output_buf).clone();
    let ss = (*screen_state).clone();
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
        ss.lock().process(&pid, &buf);

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

            // Status tracking + agent event detection (delegated to service layer)
            let (prev, now) = svc.process_pty_output(&pty_id, &data);

            // Emit status change event to frontend
            if now != prev {
                let pane_id = mux.lock().pane_id_for_pty(&pty_id);
                if let Some(ref pid) = pane_id {
                    use tauri::Emitter;
                    let _ = app.emit("pane-status-changed", serde_json::json!({
                        "pane_id": pid,
                        "status": format!("{:?}", now),
                    }));
                }
            }

            Ok(data)
        }
        Err(e) => {
            // PTY died
            tracker.lock().set_dead(&pty_id);
            let pane_id = mux.lock().pane_id_for_pty(&pty_id);
            if let Some(ref pid) = pane_id {
                use tauri::Emitter;
                let _ = app.emit("pane-status-changed", serde_json::json!({
                    "pane_id": pid,
                    "status": "Dead",
                }));

                // Cron fallback: if agent died without calling cron complete, mark as failed
                use tauri::Manager;
                if let Some(cron_state) = app.try_state::<crate::CronState>() {
                    let owner = cron_state.lock().get_pane_owner(pid).cloned();
                    if let Some((job_id, run_id)) = owner {
                        let mut store = cron_state.lock();
                        // Only save if no log exists yet (don't overwrite clean summary)
                        if store.read_log(&job_id, &run_id).is_none() {
                            let text = output_buf.lock().get_text(&pty_id).unwrap_or_default();
                            let clean = crate::api::routes::clean_terminal_output(&text);
                            let _ = store.save_log(&job_id, &run_id, &clean);
                            let _ = store.update_run_status(&job_id, &run_id, crate::cron::store::CronRunStatus::Failed);
                        }
                        store.unregister_pane(pid);
                        log::info!("Cron pane exited: job={}, run={}", job_id, run_id);
                        let _ = app.emit("cron-state-changed", ());
                    }
                }
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

    session.write_queue.send(data.into_bytes())
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
    session.write_queue.send(data)
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

/// Get captured PTY output — full untruncated history, ANSI stripped.
/// Used by ChatView to parse Claude output.
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
    let text = buf.get_full_text(&pty_id).unwrap_or_default();
    Ok(crate::api::routes::clean_terminal_output(&text))
}

/// Get current screen state (what's visible on the terminal right now).
/// Uses the VT100 screen parser — handles cursor positioning, overwrites, etc.
/// Better than get_pane_output for restoring TUI apps after remount.
#[tauri::command]
pub async fn get_pane_screen(
    screen_state: tauri::State<'_, ScreenStateHandle>,
    mux: tauri::State<'_, MuxState>,
    pane_id: String,
) -> Result<String, String> {
    let pty_id = {
        let m = mux.lock();
        m.get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
            .ok_or_else(|| "Pane not found or has no PTY".to_string())?
    };
    let ss = screen_state.lock();
    let text = ss.get_screen_text(&pty_id).unwrap_or_default();
    // Screen parser returns \n between rows, but xterm.js needs \r\n
    // to move cursor back to column 0 — otherwise each line starts
    // where the previous one ended ("staircase" effect).
    Ok(text.replace('\n', "\r\n"))
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

/// Read agent session file for a pane.
/// Returns the raw JSONL text. The frontend parses it into chat messages.
/// Supports incremental reads via `offset` — skips that many bytes and returns only new data.
/// Uses AgentRegistry to resolve session file path for the agent type.
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
        let at = pane.agent_type.clone().unwrap_or_else(|| "claude".to_string());
        (sid, cwd, at)
    };

    // Self-healing: if there's a newer session file, switch to it automatically.
    // This handles the case where Claude restarted but session_id wasn't updated yet.
    if let Some(provider) = svc.agent_registry.get(&agent_type) {
        let exclude = Vec::new();
        let max_age = Some(std::time::Duration::from_secs(86400));
        if let Some(newest) = provider.detect_session_id(&cwd, &exclude, max_age) {
            if session_id.as_deref() != Some(newest.as_str()) {
                log::info!("[read_session_jsonl] auto-switching session: {:?} -> {}", session_id, newest);
                svc.set_agent_session_id(&pane_id, Some(newest.clone()));
                svc.emit_state_changed();
                session_id = Some(newest);
            }
        }
    }

    let session_id = session_id
        .ok_or_else(|| "No agent session linked to this pane".to_string())?;

    // 2. Build path using agent provider
    let jsonl_path = svc.agent_registry.get(&agent_type)
        .and_then(|p| p.session_file_path(&cwd, &session_id))
        .ok_or_else(|| format!("No session file path for agent type: {}", agent_type))?;

    if !jsonl_path.exists() {
        return Err(format!("JSONL not found: {}", jsonl_path.display()));
    }

    // 3. Read from offset (incremental) or tail (initial fast load)
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
        // Read only the last N bytes for fast initial load
        // Prepend file_len as first line so frontend knows the offset for subsequent reads
        let start = if file_len > tail { file_len - tail } else { 0 };
        file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        // Skip first partial line (we likely landed mid-line)
        if start > 0 {
            if let Some(pos) = buf.find('\n') {
                buf = buf[pos + 1..].to_string();
            }
        }
        // Prepend offset marker so frontend can track position
        Ok(format!("__offset:{}\n{}", file_len, buf))
    } else {
        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }
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
        let exe_dir = exe.parent().unwrap_or(std::path::Path::new("."));

        // Production: skills/ next to exe
        let adjacent = exe_dir.join("skills");
        if adjacent.exists() { return adjacent; }

        // macOS .app bundle: Contents/MacOS/../Resources/skills
        #[cfg(target_os = "macos")]
        {
            let resources = exe_dir.join("../Resources/skills");
            if resources.exists() { return resources.canonicalize().unwrap_or(resources); }
        }

        // Dev: exe is in src-tauri/target/{debug,release} — project root is 3 levels up
        let dev = exe_dir.join("../../../skills");
        if dev.exists() { return dev.canonicalize().unwrap_or(dev); }

        // Dev on macOS: exe might be deeper in .app bundle during dev
        let dev_deep = exe_dir.join("../../../../skills");
        if dev_deep.exists() { return dev_deep.canonicalize().unwrap_or(dev_deep); }
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

/// Auto-update all installed skills that have a newer bundled version.
/// Returns list of skill IDs that were updated.
pub fn auto_update_skills() -> Vec<String> {
    let skills = scan_skills();
    let mut updated = Vec::new();
    for skill in skills {
        if skill.installed && skill.update_available {
            if install_skill(skill.id.clone()).is_ok() {
                updated.push(skill.id);
            }
        }
    }
    updated
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

/// Send a test notification via native OS notifications.
#[tauri::command]
pub fn test_notification(_app: tauri::AppHandle) {
    // Use osascript on macOS — works reliably in both dev and production
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("osascript")
            .args(["-e", "display notification \"Notifications are working!\" with title \"CodeTerm\""])
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = _app.notification().builder()
            .title("CodeTerm")
            .body("Notifications are working!")
            .show();
    }
}

/// Open a URL in the default browser.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/C", "start", &url]).spawn(); }
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(&url).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(&url).spawn(); }
    Ok(())
}

// ---------------------------------------------------------------------------
// Directory completion for cd
// ---------------------------------------------------------------------------

/// List directories matching a fuzzy prefix in the given path.
/// Returns directory names sorted by match quality.
#[tauri::command]
pub fn list_dir_completions(cwd: String, prefix: String) -> Result<Vec<DirCompletion>, String> {
    let dir = std::path::Path::new(&cwd);
    if !dir.exists() { return Ok(vec![]); }

    let prefix_lower = prefix.to_lowercase();
    let mut results: Vec<DirCompletion> = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let ft = entry.file_type().unwrap_or_else(|_| panic!());
        if !ft.is_dir() { continue; }

        let name = entry.file_name().to_string_lossy().to_string();
        // Skip hidden dirs
        if name.starts_with('.') { continue; }

        let name_lower = name.to_lowercase();

        // Score: 0 = exact prefix, 1 = contains, 2 = subsequence, -1 = no match
        let score = if name_lower.starts_with(&prefix_lower) {
            0
        } else if name_lower.contains(&prefix_lower) {
            1
        } else if subsequence_match(&name_lower, &prefix_lower) {
            2
        } else {
            continue; // no match
        };

        results.push(DirCompletion { name, score });
    }

    results.sort_by(|a, b| a.score.cmp(&b.score).then(a.name.len().cmp(&b.name.len())));
    // Limit to top 20
    results.truncate(20);
    Ok(results)
}

#[derive(serde::Serialize)]
pub struct DirCompletion {
    pub name: String,
    pub score: i32,
}

/// Check if `query` is a subsequence of `text`.
fn subsequence_match(text: &str, query: &str) -> bool {
    let mut qi = query.chars().peekable();
    for tc in text.chars() {
        if qi.peek() == Some(&tc) {
            qi.next();
        }
        if qi.peek().is_none() { return true; }
    }
    qi.peek().is_none()
}

// ---------------------------------------------------------------------------
// Claude Code hooks management
// ---------------------------------------------------------------------------

fn claude_settings_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

const CODETERM_HOOK_MARKER: &str = "codeterm-hook";

/// Check if a hook entry belongs to CodeTerm — either by `_source` marker or by command content.
fn is_codeterm_hook(hook_entry: &serde_json::Value) -> bool {
    hook_entry.get("hooks").and_then(|hs| hs.as_array())
        .map(|hs| hs.iter().any(|h| {
            // Check _source marker (current or legacy)
            if let Some(source) = h.get("_source").and_then(|s| s.as_str()) {
                if source == CODETERM_HOOK_MARKER || source == "codeterm" {
                    return true;
                }
            }
            // Legacy detection: check if command posts to CodeTerm API
            h.get("command").and_then(|c| c.as_str())
                .map(|cmd| {
                    let is_codeterm_url = cmd.contains("7685") || cmd.contains("7686");
                    let is_codeterm_path = cmd.contains("/api/notifications")
                        || cmd.contains("/api/panes/")
                        || cmd.contains("/hook/permission")
                        || cmd.contains("/hook/stop");
                    is_codeterm_url && is_codeterm_path
                })
                .unwrap_or(false)
        }))
        .unwrap_or(false)
}

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

    let notification_hook = json.pointer("/hooks/Notification")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_codeterm_hook))
        .unwrap_or(false);
    let stop_hook = json.pointer("/hooks/Stop")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_codeterm_hook))
        .unwrap_or(false);

    Ok(ClaudeHooksStatus {
        settings_exists: true,
        hooks_installed: notification_hook && stop_hook,
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
    let port = api_port.unwrap_or_else(|| {
        #[cfg(debug_assertions)]
        { 7686 }
        #[cfg(not(debug_assertions))]
        { 7685 }
    });

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

    // Build hook commands — cross-platform null device and error handling.
    // Use single quotes around the JSON body (works in sh/bash/zsh/powershell).
    let null_dev = crate::shell::ShellSyntax::NULL_DEV;
    // Use $CODETERM_API_PORT env var (set by PTY shell integration) with compiled-in fallback.
    // This way hooks work regardless of which CodeTerm instance (prod/dev) spawned Claude.
    let notify_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/permission -H 'Content-Type: application/json' -d '{{}}' > {null_dev} 2>&1 || true"
    );
    let stop_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/stop -H 'Content-Type: application/json' -d '{{}}' > {null_dev} 2>&1 || true"
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

    // Remove old codeterm hooks first (by _source marker or legacy command detection)
    for key in &["Notification", "Stop"] {
        if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
            arr.retain(|h| !is_codeterm_hook(h));
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
                arr.retain(|h| !is_codeterm_hook(h));
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

/// Save base64 data to a temp file and return the path.
/// Used by ChatView to send pasted images to Claude.
#[tauri::command]
pub fn save_temp_file(data: String, extension: String, name: Option<String>) -> Result<String, String> {
    use std::io::Write;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;
    // Use original filename if provided (with short UUID prefix for uniqueness)
    let prefix = &uuid::Uuid::new_v4().to_string()[..8];
    let filename = match name {
        Some(n) if !n.is_empty() => format!("{}-{}", prefix, n),
        _ => format!("codeterm-paste-{}.{}", prefix, extension),
    };
    let path = std::env::temp_dir().join(filename);
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Copy image file to system clipboard so Claude Code can read it via paste.
#[tauri::command]
pub async fn copy_image_to_clipboard(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // PowerShell single-quoted strings are literal — only ' needs escaping
        // (as ''), backslashes are NOT escape chars in PS single quotes.
        let escaped_path = path.replace("'", "''");
        let ps_script = format!(
            r#"
            Add-Type -AssemblyName System.Windows.Forms
            Add-Type -AssemblyName System.Drawing
            $img = [System.Drawing.Image]::FromFile('{escaped_path}')
            $data = New-Object System.Windows.Forms.DataObject
            $data.SetImage($img)
            $ms = New-Object System.IO.MemoryStream
            $img.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
            $ms.Position = 0
            $data.SetData('PNG', $false, $ms)
            [System.Windows.Forms.Clipboard]::SetDataObject($data, $true)
            $img.Dispose()
            "#
        );
        log::debug!("[clipboard] copying image to clipboard: {}", path);
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &ps_script])
            .output()
            .map_err(|e| format!("PowerShell launch failed: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!("[clipboard] PowerShell failed: {}", stderr);
            return Err(format!("Clipboard copy failed: {}", stderr));
        }
        log::debug!("[clipboard] image copied to clipboard successfully");
    }

    #[cfg(target_os = "macos")]
    {
        // Use osascript to copy image to clipboard
        let script = format!(
            "set the clipboard to (read (POSIX file \"{}\") as «class PNGf»)",
            path.replace("\"", "\\\"")
        );
        let output = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            // Try TIFF fallback
            let script2 = format!(
                "set the clipboard to (read (POSIX file \"{}\") as TIFF picture)",
                path.replace("\"", "\\\"")
            );
            let output2 = std::process::Command::new("osascript")
                .args(["-e", &script2])
                .output()
                .map_err(|e| e.to_string())?;
            if !output2.status.success() {
                let stderr = String::from_utf8_lossy(&output2.stderr);
                return Err(format!("Clipboard copy failed: {}", stderr));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-target", "image/png", "-i", &path])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err("xclip failed".to_string());
        }
    }

    Ok(())
}

/// Generate a new API token, save it to config, and return it.
#[tauri::command]
pub fn generate_api_token(
    state: tauri::State<'_, crate::config::ConfigState>,
    app: AppHandle,
) -> Result<String, String> {
    let token = uuid::Uuid::new_v4().to_string().replace("-", "");
    let mut cfg = state.lock().clone();
    cfg.api.token = Some(token.clone());
    crate::config::save_config_pub(&cfg)?;
    *state.lock() = cfg.clone();
    app.emit("config-changed", &cfg).ok();
    Ok(token)
}

/// Get network info (local IP, Tailscale IP, API URL).
#[tauri::command]
pub fn get_network_info(state: tauri::State<'_, crate::config::ConfigState>) -> Result<serde_json::Value, String> {
    let local_ip = crate::api::routes::local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    let tailscale_ip = crate::api::routes::detect_tailscale_ip();
    let port = state.lock().api.port;
    Ok(serde_json::json!({
        "local_ip": local_ip,
        "tailscale_ip": tailscale_ip,
        "api_port": port,
        "api_url": format!("http://{}:{}", local_ip, port),
        "tailscale_url": tailscale_ip.as_ref().map(|ip| format!("http://{}:{}", ip, port)),
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

// ---------------------------------------------------------------------------
// Cron commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn cron_list(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    workspace_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let jobs = svc.cron_list(&cron, workspace_id.as_deref());
    serde_json::to_value(jobs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_get(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let job = svc.cron_get(&cron, &id)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_create(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    name: String,
    workspace_id: String,
    schedule: String,
    task: String,
    agent: Option<String>,
    cwd: Option<String>,
    description: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = crate::cron::store::CronCreateParams {
        name,
        workspace_id,
        schedule,
        task,
        description,
        agent,
        cwd,
        role: None,
        creator_pane_id: None,
        enabled: Some(true),
        max_history: None,
    };
    let job = svc.cron_create(&cron, &wake, params)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_update(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
    name: Option<String>,
    schedule: Option<String>,
    task: Option<String>,
    cwd: Option<String>,
    description: Option<String>,
    enabled: Option<bool>,
) -> Result<serde_json::Value, String> {
    let patch = crate::cron::store::CronUpdateParams {
        name,
        description,
        schedule,
        task,
        cwd,
        role: None,
        enabled,
        max_history: None,
    };
    let job = svc.cron_update(&cron, &wake, &id, patch)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_delete(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<(), String> {
    svc.cron_delete(&cron, &wake, &id)
}

#[tauri::command]
pub fn cron_toggle(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    let job = svc.cron_enable(&cron, &wake, &id, enabled)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_trigger(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let run = svc.cron_trigger(&cron, &id)?;
    serde_json::to_value(run).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_history(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let history = svc.cron_history(&cron, &id);
    serde_json::to_value(history).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_read_log(
    cron: tauri::State<'_, crate::CronState>,
    job_id: String,
    run_id: String,
) -> Result<String, String> {
    cron.lock()
        .read_log(&job_id, &run_id)
        .ok_or_else(|| "Log not found".to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_extract_revision_valid() {
        assert_eq!(extract_revision("some content\n<!-- revision: 10 -->"), 10);
    }

    #[test]
    fn test_extract_revision_missing() {
        assert_eq!(extract_revision("no revision here"), 0);
    }

    #[test]
    fn test_extract_revision_whitespace() {
        assert_eq!(extract_revision("<!-- revision:   42   -->"), 42);
    }

    #[test]
    fn test_extract_revision_invalid_number() {
        assert_eq!(extract_revision("<!-- revision: abc -->"), 0);
    }

    #[test]
    fn test_claude_skills_dir_is_under_home() {
        let dir = claude_skills_dir();
        let home = dirs::home_dir().unwrap();
        assert!(dir.starts_with(&home));
        assert!(dir.ends_with(".claude/skills"));
    }

    #[test]
    fn test_scan_skills_with_temp_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("test-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: test\ndescription: A test skill\n---\n\n<!-- revision: 3 -->\n",
        ).unwrap();

        // scan_skills uses find_skills_dir which we can't override easily,
        // but we can test the extract_revision and parse logic directly
        let content = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        let name = content.lines()
            .find(|l| l.starts_with("name:"))
            .map(|l| l.trim_start_matches("name:").trim().to_string())
            .unwrap();
        let desc = content.lines()
            .find(|l| l.starts_with("description:"))
            .map(|l| l.trim_start_matches("description:").trim().to_string())
            .unwrap();
        let rev = extract_revision(&content);

        assert_eq!(name, "test");
        assert_eq!(desc, "A test skill");
        assert_eq!(rev, 3);
    }

    #[test]
    fn test_install_skill_nonexistent() {
        let result = install_skill("nonexistent-skill-xyz-12345".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Skill not found"));
    }

    #[test]
    fn test_uninstall_skill_nonexistent_is_ok() {
        // Uninstalling a skill that doesn't exist should succeed silently
        let result = uninstall_skill("nonexistent-skill-xyz-12345".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_hook_commands_preserve_dollar_sign() {
        // Verify that hook commands contain $CODETERM_PANE_ID (not \\$ escaped)
        let null_dev = crate::shell::ShellSyntax::NULL_DEV;
        let port = 7686u16;
        let notify_cmd = format!(
            "curl -s -X POST http://localhost:{port}/api/panes/$CODETERM_PANE_ID/hook/permission -H 'Content-Type: application/json' -d '{{}}' > {null_dev} 2>&1 || true"
        );
        // The command string itself must have literal $
        assert!(notify_cmd.contains("$CODETERM_PANE_ID"), "Command must contain $CODETERM_PANE_ID");
        assert!(!notify_cmd.contains("\\$"), "Command must NOT contain escaped \\$");

        // Verify serde_json does NOT escape $ in strings
        let json_val = serde_json::json!({"command": notify_cmd});
        let serialized = serde_json::to_string_pretty(&json_val).unwrap();
        assert!(serialized.contains("$CODETERM_PANE_ID"), "JSON serialization must preserve $");
        assert!(!serialized.contains("\\\\$"), "JSON serialization must NOT double-escape $");
        assert!(!serialized.contains("\\$"), "JSON serialization must NOT escape $");
    }

    #[test]
    fn test_install_hooks_to_temp_file() {
        // Test install_claude_hooks logic against a temporary settings file
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");

        // Write empty settings
        fs::write(&settings_path, "{}").unwrap();

        // Build hook commands (same logic as install_claude_hooks)
        let port = 7686u16;
        let null_dev = crate::shell::ShellSyntax::NULL_DEV;
        let notify_cmd = format!(
            "curl -s -X POST http://localhost:{port}/api/panes/$CODETERM_PANE_ID/hook/permission -H 'Content-Type: application/json' -d '{{}}' > {null_dev} 2>&1 || true"
        );

        let mut json: serde_json::Value = serde_json::json!({});
        json["hooks"] = serde_json::json!({});
        json["hooks"]["Notification"] = serde_json::json!([{
            "matcher": "permission_prompt",
            "hooks": [{
                "type": "command",
                "command": notify_cmd,
                "timeout": 5000,
                "_source": CODETERM_HOOK_MARKER
            }]
        }]);

        let pretty = serde_json::to_string_pretty(&json).unwrap();
        fs::write(&settings_path, &pretty).unwrap();

        // Read back and verify
        let content = fs::read_to_string(&settings_path).unwrap();
        assert!(content.contains("$CODETERM_PANE_ID"), "Written file must have $CODETERM_PANE_ID");
        assert!(!content.contains("\\$"), "Written file must NOT have escaped \\$");

        // Parse back and verify the command string
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let cmd = parsed["hooks"]["Notification"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("$CODETERM_PANE_ID"));
    }
}
