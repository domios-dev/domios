//! Codex CLI hook installer — owned by the agent-codex plugin.
//!
//! Codex's hook engine (`codex-rs/hooks/`) reads `~/.codex/hooks.json` in
//! the same Claude-shaped schema (`{"hooks":{"PreToolUse":[{matcher, hooks:[…]}], …}}`).
//! It also accepts a TOML form inside `~/.codex/config.toml`, but we
//! standardize on the JSON file because it's a single dedicated artefact:
//! easier to merge idempotently, no clobber risk on unrelated config, no
//! TOML editor dep.
//!
//! Two structural deltas vs. Claude:
//!
//! * Codex has **no `PreCompact` event** (only `PreToolUse`,
//!   `PermissionRequest`, `PostToolUse`, `SessionStart`,
//!   `UserPromptSubmit`, `Stop`). We install only PreToolUse +
//!   SessionStart + UserPromptSubmit; SessionStart already injects
//!   compaction-survival context.
//! * Codex shells hook commands via `$SHELL -lc <cmd>` on Unix and
//!   `cmd.exe /C <cmd>` on Windows. cmd.exe doesn't honor the inline
//!   `FOO=bar prog` env-prefix syntax, so we use the CLI's
//!   `--source codex` flag instead of a shell-level export. Keeps the
//!   command shape identical on every platform.
//! * The hook schema has no first-class `_source` / installer-tag field.
//!   Per `serde` defaults, unknown keys are silently ignored on parse —
//!   `_source` survives a round-trip on disk because Codex never writes
//!   the file back. That gives us a marker without schema collisions.
//!   Identification falls back to a command-substring sniff when
//!   `_source` is missing (e.g. user-edited the JSON by hand and stripped
//!   it).

use crate::plugin::cli_path::codeterm_cli_path;
use serde_json::Value;

pub const CODETERM_TOKEN_HOOK_MARKER: &str = "codeterm-hook-token-v3";

#[derive(serde::Serialize)]
pub struct CodexTokenHooksStatus {
    pub settings_exists: bool,
    pub hooks_installed: bool,
    pub pre_tool_use: bool,
    pub session_start: bool,
    pub user_prompt_submit: bool,
}

fn codex_hooks_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".codex").join("hooks.json"))
}

fn is_codeterm_hook(hook_entry: &Value) -> bool {
    let arr = match hook_entry.get("hooks").and_then(|h| h.as_array()) {
        Some(a) => a,
        None => return false,
    };
    arr.iter().any(|h| {
        if let Some(src) = h.get("_source").and_then(|s| s.as_str()) {
            if src == CODETERM_TOKEN_HOOK_MARKER || src.starts_with("codeterm-hook") {
                return true;
            }
        }
        // Fallback: command-string sniff. If the user (or Codex's
        // round-trip) drops `_source`, identify our entries by the
        // distinctive `--source codex` argument the installer always
        // emits in front of `hook <event>`.
        h.get("command")
            .and_then(|c| c.as_str())
            .map(|cmd| cmd.contains("--source codex") && cmd.contains(" hook "))
            .unwrap_or(false)
    })
}

fn read_hooks_json(path: &std::path::Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if content.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse codex hooks.json: {}", e))
}

fn write_hooks_json(path: &std::path::Path, json: &Value) -> Result<(), String> {
    let pretty = serde_json::to_string_pretty(json).map_err(|e| e.to_string())?;
    std::fs::write(path, pretty).map_err(|e| e.to_string())
}

pub fn check_token_hooks() -> Result<CodexTokenHooksStatus, String> {
    let path = codex_hooks_path()?;
    if !path.exists() {
        return Ok(CodexTokenHooksStatus {
            settings_exists: false,
            hooks_installed: false,
            pre_tool_use: false,
            session_start: false,
            user_prompt_submit: false,
        });
    }
    let json = read_hooks_json(&path)?;
    let any_present = |key: &str| -> bool {
        json.pointer(&format!("/hooks/{}", key))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(is_codeterm_hook))
            .unwrap_or(false)
    };
    let pre_tool_use = any_present("PreToolUse");
    let session_start = any_present("SessionStart");
    let user_prompt_submit = any_present("UserPromptSubmit");
    Ok(CodexTokenHooksStatus {
        settings_exists: true,
        hooks_installed: pre_tool_use && session_start && user_prompt_submit,
        pre_tool_use,
        session_start,
        user_prompt_submit,
    })
}

pub fn install_token_hooks() -> Result<(), String> {
    let path = codex_hooks_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut json = read_hooks_json(&path)?;

    let cli = codeterm_cli_path();
    let make_entry = |matcher: Option<&str>, event: &str, timeout_sec: u64| -> Value {
        // Double-quoted CLI path is the form that works in BOTH /bin/sh
        // (Codex on Unix uses `$SHELL -lc <cmd>`) AND cmd.exe (Codex on
        // Windows uses `cmd.exe /C <cmd>`, verified in
        // references/codex/codex-rs/hooks/src/engine/command_runner.rs).
        // cmd.exe does NOT strip single quotes — it treats them as
        // literal characters and the invocation fails. Double quotes
        // work in both shells. `--source codex` overrides the CLI's env
        // dispatch so we don't need a shell-level env prefix (which
        // also breaks under cmd.exe).
        let command = format!("\"{cli}\" hook --source codex {event}");
        let mut entry = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": command,
                "timeout": timeout_sec,
                "_source": CODETERM_TOKEN_HOOK_MARKER,
            }]
        });
        // Codex's UserPromptSubmit / Stop dispatch ignores matchers
        // (verified in HOOK_EVENT_NAMES_WITH_MATCHERS, codex-rs/hooks/src/lib.rs).
        // Omit the field on those events so we don't carry inert
        // configuration that future schema validators might reject.
        if let Some(m) = matcher {
            entry["matcher"] = Value::String(m.to_string());
        }
        entry
    };

    if json.get("hooks").is_none() {
        json["hooks"] = serde_json::json!({});
    }
    let hooks = json["hooks"]
        .as_object_mut()
        .ok_or("hooks is not an object")?;

    for key in &["PreToolUse", "SessionStart", "UserPromptSubmit"] {
        if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
            arr.retain(|h| !is_codeterm_hook(h));
        }
        if !hooks.contains_key(*key) {
            hooks.insert((*key).into(), serde_json::json!([]));
        }
    }
    // Codex matchers default to "match anything" when omitted; for
    // PreToolUse we restrict to Bash to mirror Claude's shape (regex —
    // "Bash" matches itself).
    // PreToolUse matcher is a regex — anchor it so `Bash` doesn't
    // accidentally match a future `BashTool` or similar.
    hooks["PreToolUse"]
        .as_array_mut()
        .unwrap()
        .push(make_entry(Some("^Bash$"), "pre-tool-use", 5));
    hooks["SessionStart"]
        .as_array_mut()
        .unwrap()
        .push(make_entry(None, "session-start", 5));
    hooks["UserPromptSubmit"]
        .as_array_mut()
        .unwrap()
        .push(make_entry(None, "user-prompt-submit", 5));

    write_hooks_json(&path, &json)?;
    log::info!("Codex token-tool hooks installed at {:?}", path);
    Ok(())
}

pub fn uninstall_token_hooks() -> Result<(), String> {
    let path = codex_hooks_path()?;
    if !path.exists() {
        return Ok(());
    }
    let mut json = read_hooks_json(&path)?;
    if let Some(hooks) = json.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        for key in &["PreToolUse", "SessionStart", "UserPromptSubmit"] {
            if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
                arr.retain(|h| !is_codeterm_hook(h));
            }
        }
    }
    write_hooks_json(&path, &json)?;
    log::info!("Codex token-tool hooks removed");
    Ok(())
}

pub fn ensure_token_hooks_installed() {
    match check_token_hooks() {
        Ok(status) if status.hooks_installed => {
            log::debug!("[startup] codex token hooks already installed");
        }
        Ok(_) | Err(_) => {
            if let Err(e) = install_token_hooks() {
                log::warn!("[startup] failed to auto-install codex token hooks: {}", e);
            } else {
                log::info!("[startup] codex token hooks auto-installed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("codeterm-codex-hook-{}-{}-{}", name, pid, n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // HOME is process-global; tests that mutate it must serialize.
    fn home_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[test]
    fn marker_recognized_by_source_field() {
        let entry = serde_json::json!({
            "matcher": "Bash",
            "hooks": [{"type":"command","command":"x","_source":CODETERM_TOKEN_HOOK_MARKER}]
        });
        assert!(is_codeterm_hook(&entry));
    }

    #[test]
    fn marker_recognized_by_command_fallback() {
        let entry = serde_json::json!({
            "hooks": [{"type":"command","command":"\"/usr/bin/codeterm\" hook --source codex session-start"}]
        });
        assert!(is_codeterm_hook(&entry));
    }

    #[test]
    fn unrelated_hook_not_recognized() {
        let entry = serde_json::json!({
            "matcher": "",
            "hooks": [{"type":"command","command":"echo hi"}]
        });
        assert!(!is_codeterm_hook(&entry));
    }

    #[test]
    fn install_then_check_then_uninstall_roundtrip() {
        let _guard = home_lock().lock().unwrap_or_else(|p| p.into_inner());
        let dir = tempdir("rt");
        let prev_home = std::env::var("HOME").ok();
        let prev_codex = std::env::var("CODEX_HOME").ok();
        std::env::set_var("HOME", &dir);
        std::env::remove_var("CODEX_HOME");
        let res = (|| -> Result<(), String> {
            install_token_hooks()?;
            let st = check_token_hooks()?;
            assert!(st.settings_exists);
            assert!(st.pre_tool_use);
            assert!(st.session_start);
            assert!(st.user_prompt_submit);
            assert!(st.hooks_installed);
            uninstall_token_hooks()?;
            let st2 = check_token_hooks()?;
            assert!(!st2.hooks_installed);
            assert!(!st2.pre_tool_use);
            Ok(())
        })();
        if let Some(h) = prev_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(c) = prev_codex {
            std::env::set_var("CODEX_HOME", c);
        }
        let _ = std::fs::remove_dir_all(&dir);
        res.unwrap();
    }

    #[test]
    fn install_is_idempotent_no_duplicate_entries() {
        let _guard = home_lock().lock().unwrap_or_else(|p| p.into_inner());
        let dir = tempdir("idem");
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &dir);
        let res = (|| -> Result<(), String> {
            install_token_hooks()?;
            install_token_hooks()?;
            let raw = std::fs::read_to_string(dir.join(".codex/hooks.json"))
                .map_err(|e| e.to_string())?;
            let json: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            for key in &["PreToolUse", "SessionStart", "UserPromptSubmit"] {
                let arr = json.pointer(&format!("/hooks/{}", key))
                    .and_then(|v| v.as_array())
                    .unwrap();
                let ours: usize = arr.iter().filter(|h| is_codeterm_hook(h)).count();
                assert_eq!(ours, 1, "{} should have exactly 1 codeterm hook, got {}", key, ours);
            }
            Ok(())
        })();
        if let Some(h) = prev_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
        let _ = std::fs::remove_dir_all(&dir);
        res.unwrap();
    }

    #[test]
    fn install_preserves_user_authored_hooks() {
        let _guard = home_lock().lock().unwrap_or_else(|p| p.into_inner());
        let dir = tempdir("preserve");
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &dir);
        let res = (|| -> Result<(), String> {
            std::fs::create_dir_all(dir.join(".codex")).map_err(|e| e.to_string())?;
            let user_authored = serde_json::json!({
                "hooks": {
                    "PreToolUse": [{
                        "matcher": "Edit",
                        "hooks": [{"type":"command","command":"my-script.sh","timeout":3}]
                    }]
                }
            });
            std::fs::write(
                dir.join(".codex/hooks.json"),
                serde_json::to_string_pretty(&user_authored).unwrap(),
            )
            .map_err(|e| e.to_string())?;
            install_token_hooks()?;
            let raw = std::fs::read_to_string(dir.join(".codex/hooks.json"))
                .map_err(|e| e.to_string())?;
            let json: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            let pre = json.pointer("/hooks/PreToolUse")
                .and_then(|v| v.as_array())
                .unwrap();
            assert_eq!(pre.len(), 2, "must keep user hook + add ours");
            let user_kept = pre.iter().any(|e| {
                e.get("matcher").and_then(Value::as_str) == Some("Edit")
                    && e.get("hooks")
                        .and_then(|h| h.as_array())
                        .and_then(|a| a.first())
                        .and_then(|h0| h0.get("command"))
                        .and_then(Value::as_str)
                        == Some("my-script.sh")
            });
            assert!(user_kept);
            Ok(())
        })();
        if let Some(h) = prev_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
        let _ = std::fs::remove_dir_all(&dir);
        res.unwrap();
    }
}
