//! Claude Code hook installer — owned by the agent-claude plugin.
//!
//! Two distinct hook families live here:
//!
//! * **v2 notification hooks** — wire Claude Code's `Notification` /
//!   `Stop` / `PreCompact` / `SessionStart` events to CodeTerm's REST API
//!   so the host learns about prompt prompts, agent quiescence, and
//!   session changes.
//! * **token-saving hooks (v3)** — wire `PreToolUse` / `SessionStart` /
//!   `UserPromptSubmit` / `PreCompact` to the `codeterm hook <event>`
//!   subcommand, which truncates noisy Bash output and re-injects
//!   compaction-survival context.
//!
//! Both families coexist (different `_source` markers, different command
//! shapes). Auto-install happens at every daemon start via
//! `ensure_v2_hooks_installed` / `ensure_token_hooks_installed`. The
//! Tauri command shims in `commands/hooks.rs` thinly delegate here.

use crate::plugin::cli_path::codeterm_cli_path;
use crate::shell::ShellSyntax;
use serde_json::Value;

// ── Markers ──────────────────────────────────────────────────────────

// v2 = hooks carry `Authorization: Bearer $CODETERM_API_TOKEN` and rely on
// auth_middleware instead of the old loopback-exemption path. Any older
// marker is treated as upgradable so we overwrite the pre-v2 curl commands.
pub const CODETERM_HOOK_MARKER: &str = "codeterm-hook-v2";
pub const CODETERM_TOKEN_HOOK_MARKER: &str = "codeterm-hook-token-v3";

// ── Status types (mirrors the Tauri command return shape) ────────────

#[derive(serde::Serialize)]
pub struct ClaudeHooksStatus {
    pub settings_exists: bool,
    pub hooks_installed: bool,
    pub notification_hook: bool,
    pub stop_hook: bool,
    pub precompact_hook: bool,
    pub session_start_hook: bool,
}

#[derive(serde::Serialize)]
pub struct ClaudeTokenHooksStatus {
    pub settings_exists: bool,
    pub hooks_installed: bool,
    pub pre_tool_use: bool,
    pub session_start: bool,
    pub user_prompt_submit: bool,
    pub pre_compact: bool,
}

// ── Common path + identification helpers ─────────────────────────────

fn claude_settings_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

fn is_codeterm_v2_hook(hook_entry: &Value) -> bool {
    hook_entry
        .get("hooks")
        .and_then(|hs| hs.as_array())
        .map(|hs| {
            hs.iter().any(|h| {
                if let Some(source) = h.get("_source").and_then(|s| s.as_str()) {
                    if source.starts_with("codeterm-hook") || source == "codeterm" {
                        return true;
                    }
                }
                h.get("command")
                    .and_then(|c| c.as_str())
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
            })
        })
        .unwrap_or(false)
}

fn is_current_v2_hook(hook_entry: &Value) -> bool {
    hook_entry
        .get("hooks")
        .and_then(|hs| hs.as_array())
        .map(|hs| {
            hs.iter().any(|h| {
                h.get("_source")
                    .and_then(|s| s.as_str())
                    .map(|src| src == CODETERM_HOOK_MARKER)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn is_token_hook(hook_entry: &Value) -> bool {
    hook_entry
        .get("hooks")
        .and_then(|hs| hs.as_array())
        .map(|hs| {
            hs.iter().any(|h| {
                h.get("_source")
                    .and_then(|s| s.as_str())
                    .map(|src| src == CODETERM_TOKEN_HOOK_MARKER)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn read_settings(path: &std::path::Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if content.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&content).map_err(|e| format!("Failed to parse settings.json: {}", e))
}

fn write_settings_pretty(path: &std::path::Path, json: &Value) -> Result<(), String> {
    let pretty = serde_json::to_string_pretty(json).map_err(|e| e.to_string())?;
    std::fs::write(path, pretty).map_err(|e| e.to_string())
}

// ── v2 notification hooks (curl → REST API) ──────────────────────────

pub fn check_v2_hooks() -> Result<ClaudeHooksStatus, String> {
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
    let json = read_settings(&path)?;
    let any_present = |key: &str| -> bool {
        json.pointer(&format!("/hooks/{}", key))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(is_current_v2_hook))
            .unwrap_or(false)
    };
    let notification_hook = any_present("Notification");
    let stop_hook = any_present("Stop");
    let precompact_hook = any_present("PreCompact");
    let session_start_hook = any_present("SessionStart");
    Ok(ClaudeHooksStatus {
        settings_exists: true,
        hooks_installed: notification_hook && stop_hook && precompact_hook && session_start_hook,
        notification_hook,
        stop_hook,
        precompact_hook,
        session_start_hook,
    })
}

pub fn install_v2_hooks(api_port: Option<u16>) -> Result<(), String> {
    let path = claude_settings_path()?;
    let port = api_port.unwrap_or_else(|| {
        #[cfg(debug_assertions)]
        {
            7686
        }
        #[cfg(not(debug_assertions))]
        {
            7685
        }
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut json = read_settings(&path)?;
    let null_dev = ShellSyntax::NULL_DEV;
    let auth_h = "-H \"Authorization: Bearer ${CODETERM_API_TOKEN}\"";
    let notify_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/permission -H 'Content-Type: application/json' {auth_h} -d '{{}}' > {null_dev} 2>&1 || true"
    );
    let stop_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/stop -H 'Content-Type: application/json' {auth_h} -d '{{}}' > {null_dev} 2>&1 || true"
    );
    let precompact_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/mem/flush -H 'Content-Type: application/json' {auth_h} -d '{{\"pane\":\"'\"$CODETERM_PANE_ID\"'\",\"title\":\"pre-compact\"}}' > {null_dev} 2>&1 || true"
    );
    let session_start_cmd = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{port}}}/api/panes/$CODETERM_PANE_ID/hook/session_start -H 'Content-Type: application/json' {auth_h} --data-binary @- > {null_dev} 2>&1 || true"
    );

    if json.get("hooks").is_none() {
        json["hooks"] = serde_json::json!({});
    }

    let make = |matcher: &str, command: &str, timeout: u64| -> Value {
        serde_json::json!({
            "matcher": matcher,
            "hooks": [{
                "type": "command",
                "command": command,
                "timeout": timeout,
                "_source": CODETERM_HOOK_MARKER,
            }]
        })
    };

    let hooks = json["hooks"]
        .as_object_mut()
        .ok_or("hooks is not an object")?;
    for key in &["Notification", "Stop", "PreCompact", "SessionStart"] {
        if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
            arr.retain(|h| !is_codeterm_v2_hook(h));
        }
        if !hooks.contains_key(*key) {
            hooks.insert((*key).into(), serde_json::json!([]));
        }
    }
    hooks["Notification"]
        .as_array_mut()
        .unwrap()
        .push(make("permission_prompt", &notify_cmd, 5000));
    hooks["Stop"]
        .as_array_mut()
        .unwrap()
        .push(make("", &stop_cmd, 5000));
    hooks["PreCompact"]
        .as_array_mut()
        .unwrap()
        .push(make("", &precompact_cmd, 10000));
    hooks["SessionStart"]
        .as_array_mut()
        .unwrap()
        .push(make("", &session_start_cmd, 5000));

    write_settings_pretty(&path, &json)?;
    log::info!("Claude Code hooks installed at {:?}", path);
    Ok(())
}

pub fn uninstall_v2_hooks() -> Result<(), String> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok(());
    }
    let mut json = read_settings(&path)?;
    if let Some(hooks) = json.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        for key in &["Notification", "Stop"] {
            if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
                arr.retain(|h| !is_codeterm_v2_hook(h));
            }
        }
    }
    write_settings_pretty(&path, &json)?;
    log::info!("Claude Code hooks removed");
    Ok(())
}

pub fn ensure_v2_hooks_installed() {
    match check_v2_hooks() {
        Ok(status) if status.hooks_installed => {
            log::debug!("[startup] claude hooks already installed");
        }
        Ok(_) | Err(_) => {
            if let Err(e) = install_v2_hooks(None) {
                log::warn!("[startup] failed to auto-install claude hooks: {}", e);
            } else {
                log::info!("[startup] claude hooks auto-installed (Notification, Stop, PreCompact, SessionStart)");
            }
        }
    }
}

// ── Token-saving hooks (v3) ──────────────────────────────────────────

pub fn check_token_hooks() -> Result<ClaudeTokenHooksStatus, String> {
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
    let json = read_settings(&path)?;
    let any_present = |key: &str| -> bool {
        json.pointer(&format!("/hooks/{}", key))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(is_token_hook))
            .unwrap_or(false)
    };
    let pre_tool_use = any_present("PreToolUse");
    let session_start = any_present("SessionStart");
    let user_prompt_submit = any_present("UserPromptSubmit");
    let pre_compact = any_present("PreCompact");
    Ok(ClaudeTokenHooksStatus {
        settings_exists: true,
        hooks_installed: pre_tool_use && session_start && user_prompt_submit && pre_compact,
        pre_tool_use,
        session_start,
        user_prompt_submit,
        pre_compact,
    })
}

pub fn install_token_hooks() -> Result<(), String> {
    let path = claude_settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut json = read_settings(&path)?;

    let cli = codeterm_cli_path();
    let make_entry = |matcher: &str, event: &str, timeout_ms: u64| -> Value {
        // Single-quoted forward-slash CLI path is the form git-bash on Windows
        // accepts; Claude Code spawns hooks under git-bash there. (Claude on
        // Unix runs them via /bin/sh which also accepts single quotes.)
        // CODETERM_HOOK_SOURCE is left unset — the CLI defaults to claude.
        serde_json::json!({
            "matcher": matcher,
            "hooks": [{
                "type": "command",
                "command": format!("'{cli}' hook {event}"),
                "timeout": timeout_ms,
                "_source": CODETERM_TOKEN_HOOK_MARKER,
            }]
        })
    };

    if json.get("hooks").is_none() {
        json["hooks"] = serde_json::json!({});
    }
    let hooks = json["hooks"]
        .as_object_mut()
        .ok_or("hooks is not an object")?;
    for key in &["PreToolUse", "SessionStart", "UserPromptSubmit", "PreCompact"] {
        if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
            arr.retain(|h| !is_token_hook(h));
        }
        if !hooks.contains_key(*key) {
            hooks.insert((*key).into(), serde_json::json!([]));
        }
    }
    hooks["PreToolUse"]
        .as_array_mut()
        .unwrap()
        .push(make_entry("Bash", "pre-tool-use", 5000));
    hooks["SessionStart"]
        .as_array_mut()
        .unwrap()
        .push(make_entry("", "session-start", 5000));
    hooks["UserPromptSubmit"]
        .as_array_mut()
        .unwrap()
        .push(make_entry("", "user-prompt-submit", 5000));
    hooks["PreCompact"]
        .as_array_mut()
        .unwrap()
        .push(make_entry("", "pre-compact", 5000));

    write_settings_pretty(&path, &json)?;
    log::info!("Claude token-tool hooks installed at {:?}", path);
    Ok(())
}

pub fn uninstall_token_hooks() -> Result<(), String> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok(());
    }
    let mut json = read_settings(&path)?;
    if let Some(hooks) = json.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        for key in &["PreToolUse", "SessionStart", "UserPromptSubmit", "PreCompact"] {
            if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
                arr.retain(|h| !is_token_hook(h));
            }
        }
    }
    write_settings_pretty(&path, &json)?;
    log::info!("Claude token-tool hooks removed");
    Ok(())
}

pub fn ensure_token_hooks_installed() {
    match check_token_hooks() {
        Ok(status) if status.hooks_installed => {
            log::debug!("[startup] claude token hooks already installed");
        }
        Ok(_) | Err(_) => {
            if let Err(e) = install_token_hooks() {
                log::warn!("[startup] failed to auto-install claude token hooks: {}", e);
            } else {
                log::info!("[startup] claude token hooks auto-installed");
            }
        }
    }
}
