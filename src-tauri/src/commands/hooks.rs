use super::{ClaudeHooksStatus, ClaudeTokenHooksStatus};

fn claude_settings_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

// v2 = hooks carry `Authorization: Bearer $CODETERM_API_TOKEN` and rely on
// auth_middleware instead of the old loopback-exemption path. Any older
// marker is treated as upgradable so we overwrite the pre-v2 curl commands.
pub(super) const CODETERM_HOOK_MARKER: &str = "codeterm-hook-v2";
pub(super) const CODETERM_TOKEN_HOOK_MARKER: &str = "codeterm-hook-token-v3";

/// True when a hook entry was installed by CodeTerm at any version — used on
/// install to REMOVE stale entries before writing the current ones. Accepts
/// any historical marker plus a URL-based legacy fallback.
fn is_codeterm_hook(hook_entry: &serde_json::Value) -> bool {
    hook_entry.get("hooks").and_then(|hs| hs.as_array())
        .map(|hs| hs.iter().any(|h| {
            if let Some(source) = h.get("_source").and_then(|s| s.as_str()) {
                if source.starts_with("codeterm-hook") || source == "codeterm" {
                    return true;
                }
            }
            h.get("command").and_then(|c| c.as_str())
                .map(|cmd| {
                    let is_codeterm_url = cmd.contains("7685") || cmd.contains("7686");
                    let is_codeterm_path = cmd.contains("/api/notifications")
                        || cmd.contains("/api/panes/")
                        || cmd.contains("/hook/permission")
                        || cmd.contains("/hook/stop")
                        || cmd.contains("/hook/session_start")
                        || cmd.contains("/api/mem/flush");
                    is_codeterm_url && is_codeterm_path
                })
                .unwrap_or(false)
        }))
        .unwrap_or(false)
}

/// True only when the hook matches the CURRENT installer output. Drives the
/// `hooks_installed` bit so we force-upgrade stale pre-v2 entries (which
/// lacked the auth header and relied on the dropped exemption).
fn is_current_codeterm_hook(hook_entry: &serde_json::Value) -> bool {
    hook_entry.get("hooks").and_then(|hs| hs.as_array())
        .map(|hs| hs.iter().any(|h| {
            h.get("_source").and_then(|s| s.as_str())
                .map(|src| src == CODETERM_HOOK_MARKER)
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
            precompact_hook: false,
            session_start_hook: false,
        });
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings.json: {}", e))?;

    let notification_hook = json.pointer("/hooks/Notification")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let stop_hook = json.pointer("/hooks/Stop")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let precompact_hook = json.pointer("/hooks/PreCompact")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let session_start_hook = json.pointer("/hooks/SessionStart")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);

    Ok(ClaudeHooksStatus {
        settings_exists: true,
        hooks_installed: notification_hook && stop_hook && precompact_hook && session_start_hook,
        notification_hook,
        stop_hook,
        precompact_hook,
        session_start_hook,
    })
}

pub fn ensure_claude_hooks_installed() {
    match check_claude_hooks() {
        Ok(status) if status.hooks_installed => {
            log::debug!("[startup] claude hooks already installed");
        }
        Ok(_) | Err(_) => {
            if let Err(e) = install_claude_hooks(None) {
                log::warn!("[startup] failed to auto-install claude hooks: {}", e);
            } else {
                log::info!("[startup] claude hooks auto-installed (Notification, Stop, PreCompact, SessionStart)");
            }
        }
    }
}

#[tauri::command]
pub fn install_claude_hooks(api_port: Option<u16>) -> Result<(), String> {
    let path = claude_settings_path()?;
    let port = api_port.unwrap_or_else(|| {
        #[cfg(debug_assertions)]
        { 7686 }
        #[cfg(not(debug_assertions))]
        { 7685 }
    });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut json: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))?
    } else {
        serde_json::json!({})
    };

    let null_dev = crate::shell::ShellSyntax::NULL_DEV;
    // Hook callbacks share one auth header so the endpoints can stay behind
    // auth_middleware and don't need a loopback/LAN exemption. CodeTerm sets
    // $CODETERM_API_TOKEN in the PTY env at spawn time.
    let auth_h = "-H \"Authorization: Bearer ${CODETERM_API_TOKEN}\"";
    let notify_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/permission -H 'Content-Type: application/json' {auth_h} -d '{{}}' > {null_dev} 2>&1 || true"
    );
    let stop_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/stop -H 'Content-Type: application/json' {auth_h} -d '{{}}' > {null_dev} 2>&1 || true"
    );
    // flush PTY scrollback before summarization destroys it
    let precompact_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/mem/flush -H 'Content-Type: application/json' {auth_h} -d '{{\"pane\":\"'\"$CODETERM_PANE_ID\"'\",\"title\":\"pre-compact\"}}' > {null_dev} 2>&1 || true"
    );
    // Forward Claude's SessionStart JSON (delivered on stdin) to CodeTerm so we
    // learn the new session_id when the user runs /new, /clear, or starts fresh.
    let session_start_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/session_start -H 'Content-Type: application/json' {auth_h} --data-binary @- > {null_dev} 2>&1 || true"
    );

    if json.get("hooks").is_none() {
        json["hooks"] = serde_json::json!({});
    }

    let notif_hook = serde_json::json!({
        "matcher": "permission_prompt",
        "hooks": [{
            "type": "command",
            "command": notify_cmd,
            "timeout": 5000,
            "_source": CODETERM_HOOK_MARKER
        }]
    });

    let stop_hook = serde_json::json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": stop_cmd,
            "timeout": 5000,
            "_source": CODETERM_HOOK_MARKER
        }]
    });

    let precompact_hook = serde_json::json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": precompact_cmd,
            "timeout": 10000,
            "_source": CODETERM_HOOK_MARKER
        }]
    });

    let session_start_hook = serde_json::json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": session_start_cmd,
            "timeout": 5000,
            "_source": CODETERM_HOOK_MARKER
        }]
    });

    let hooks = json["hooks"].as_object_mut()
        .ok_or("hooks is not an object")?;

    for key in &["Notification", "Stop", "PreCompact", "SessionStart"] {
        if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
            arr.retain(|h| !is_codeterm_hook(h));
        }
    }

    if !hooks.contains_key("Notification") {
        hooks.insert("Notification".into(), serde_json::json!([]));
    }
    hooks["Notification"].as_array_mut().unwrap().push(notif_hook);

    if !hooks.contains_key("Stop") {
        hooks.insert("Stop".into(), serde_json::json!([]));
    }
    hooks["Stop"].as_array_mut().unwrap().push(stop_hook);

    if !hooks.contains_key("PreCompact") {
        hooks.insert("PreCompact".into(), serde_json::json!([]));
    }
    hooks["PreCompact"].as_array_mut().unwrap().push(precompact_hook);

    if !hooks.contains_key("SessionStart") {
        hooks.insert("SessionStart".into(), serde_json::json!([]));
    }
    hooks["SessionStart"].as_array_mut().unwrap().push(session_start_hook);

    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    log::info!("Claude Code hooks installed at {:?}", path);
    Ok(())
}

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

fn is_token_hook(hook_entry: &serde_json::Value) -> bool {
    hook_entry.get("hooks").and_then(|hs| hs.as_array())
        .map(|hs| hs.iter().any(|h| {
            h.get("_source").and_then(|s| s.as_str())
                .map(|src| src == CODETERM_TOKEN_HOOK_MARKER)
                .unwrap_or(false)
        }))
        .unwrap_or(false)
}

fn codeterm_cli_path() -> String {
    if let Some(home) = dirs::home_dir() {
        let exe = if cfg!(windows) { "codeterm.exe" } else { "codeterm" };
        let p = home.join(".codeterm").join("bin").join(exe);
        if let Some(s) = p.to_str() {
            return s.to_string();
        }
    }
    "codeterm".to_string()
}

#[tauri::command]
pub fn check_claude_token_hooks() -> Result<ClaudeTokenHooksStatus, String> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok(ClaudeTokenHooksStatus {
            settings_exists: false,
            hooks_installed: false,
            pre_tool_use: false,
            session_start: false,
            user_prompt_submit: false,
            pre_compact: false,
        });
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings.json: {}", e))?;
    let pre_tool_use = json.pointer("/hooks/PreToolUse")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_token_hook))
        .unwrap_or(false);
    let session_start = json.pointer("/hooks/SessionStart")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_token_hook))
        .unwrap_or(false);
    let user_prompt_submit = json.pointer("/hooks/UserPromptSubmit")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_token_hook))
        .unwrap_or(false);
    let pre_compact = json.pointer("/hooks/PreCompact")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_token_hook))
        .unwrap_or(false);
    Ok(ClaudeTokenHooksStatus {
        settings_exists: true,
        hooks_installed: pre_tool_use && session_start && user_prompt_submit && pre_compact,
        pre_tool_use,
        session_start,
        user_prompt_submit,
        pre_compact,
    })
}

#[tauri::command]
pub fn install_claude_token_hooks() -> Result<(), String> {
    let path = claude_settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut json: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))?
    } else {
        serde_json::json!({})
    };

    let cli = codeterm_cli_path();
    let make_entry = |matcher: &str, event: &str, timeout_ms: u64| -> serde_json::Value {
        serde_json::json!({
            "matcher": matcher,
            "hooks": [{
                "type": "command",
                "command": format!("\"{cli}\" hook {event}"),
                "timeout": timeout_ms,
                "_source": CODETERM_TOKEN_HOOK_MARKER,
            }]
        })
    };

    if json.get("hooks").is_none() {
        json["hooks"] = serde_json::json!({});
    }
    let hooks = json["hooks"].as_object_mut().ok_or("hooks is not an object")?;

    for key in &["PreToolUse", "SessionStart", "UserPromptSubmit", "PreCompact"] {
        if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
            arr.retain(|h| !is_token_hook(h));
        }
        if !hooks.contains_key(*key) {
            hooks.insert((*key).into(), serde_json::json!([]));
        }
    }

    hooks["PreToolUse"].as_array_mut().unwrap().push(make_entry("Bash", "pre-tool-use", 5000));
    hooks["SessionStart"].as_array_mut().unwrap().push(make_entry("", "session-start", 5000));
    hooks["UserPromptSubmit"].as_array_mut().unwrap().push(make_entry("", "user-prompt-submit", 5000));
    hooks["PreCompact"].as_array_mut().unwrap().push(make_entry("", "pre-compact", 5000));

    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    log::info!("Claude token-tool hooks installed at {:?}", path);
    Ok(())
}

#[tauri::command]
pub fn uninstall_claude_token_hooks() -> Result<(), String> {
    let path = claude_settings_path()?;
    if !path.exists() { return Ok(()); }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings.json: {}", e))?;
    if let Some(hooks) = json.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        for key in &["PreToolUse", "SessionStart", "UserPromptSubmit", "PreCompact"] {
            if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
                arr.retain(|h| !is_token_hook(h));
            }
        }
    }
    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    log::info!("Claude token-tool hooks removed");
    Ok(())
}

pub fn ensure_claude_token_hooks_installed() {
    match check_claude_token_hooks() {
        Ok(status) if status.hooks_installed => {
            log::debug!("[startup] claude token hooks already installed");
        }
        Ok(_) | Err(_) => {
            if let Err(e) = install_claude_token_hooks() {
                log::warn!("[startup] failed to auto-install claude token hooks: {}", e);
            } else {
                log::info!("[startup] claude token hooks auto-installed");
            }
        }
    }
}
