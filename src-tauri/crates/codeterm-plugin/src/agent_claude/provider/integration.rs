//! CodeTerm integration with Claude Code — hooks installed into
//! `~/.claude/settings.json` so Claude POSTs back to CodeTerm on
//! permission prompts, task completion, and session start. Also owns the
//! project-rule (`~/.claude/rules/codeterm.md`) install/check logic.
//!
//! Everything here was previously in `commands/hooks.rs` and
//! `commands/agent.rs` with hardcoded `claude_*` names. It moved into
//! the Claude provider as part of the abstraction pass so adding a new
//! provider doesn't require touching `commands/`.

use crate::prompts::CODETERM_RULE;
use crate::provider::IntegrationInstallOptions;
use serde_json::Value as Json;

// v3 resolves the callback from the launched Tab's environment, so prod and
// isolated dev instances can safely share Claude's global settings file.
const CODETERM_HOOK_MARKER: &str = "codeterm-hook-v3";

fn claude_settings_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

fn claude_rule_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".claude").join("rules").join("codeterm.md"))
}

fn backup_path(path: &std::path::Path) -> std::path::PathBuf {
    path.with_file_name(format!(
        "{}.codeterm-backup",
        path.file_name().and_then(|name| name.to_str()).unwrap_or("settings.json")
    ))
}

fn write_json_atomic(path: &std::path::Path, json: &Json) -> Result<(), String> {
    let pretty = serde_json::to_string_pretty(json).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.codeterm-tmp");
    std::fs::write(&temporary, pretty).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

/// True when a hook entry was installed by CodeTerm at any version — used on
/// install to REMOVE stale entries before writing the current ones.
fn is_codeterm_hook(hook_entry: &Json) -> bool {
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
                            || cmd.contains("/api/tabs/")
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

/// True only when the hook matches the CURRENT installer output. Drives
/// the `hooks_installed` bit so we force-upgrade stale pre-v2 entries.
fn is_current_codeterm_hook(hook_entry: &Json) -> bool {
    hook_entry
        .get("hooks")
        .and_then(|hs| hs.as_array())
        .map(|hs| {
            hs.iter().any(|h| {
                let current_marker = h
                    .get("_source")
                    .and_then(|s| s.as_str())
                    .map(|src| src == CODETERM_HOOK_MARKER)
                    .unwrap_or(false);
                let current_tab_contract = h
                    .get("command")
                    .and_then(|command| command.as_str())
                    .map(|command| {
                        command.contains("CODETERM_TAB_ID")
                            && command.contains("CODETERM_API_URL")
                            && command.contains("CODETERM_API_TOKEN")
                            && !command.contains("CODETERM_API_PORT")
                            && !command.contains("CODETERM_PANE_ID")
                            && !command.contains("/api/panes/")
                    })
                    .unwrap_or(false);
                current_marker && current_tab_contract
            })
        })
        .unwrap_or(false)
}

fn is_current_session_start_hook(hook_entry: &Json) -> bool {
    hook_entry
        .get("hooks")
        .and_then(|hooks| hooks.as_array())
        .map(|hooks| {
            hooks.iter().any(|hook| {
                let current_marker = hook.get("_source").and_then(|source| source.as_str())
                    == Some(CODETERM_HOOK_MARKER);
                let current_command = hook
                    .get("command")
                    .and_then(|command| command.as_str())
                    .map(|command| {
                        command.contains("CODETERM_TAB_ID")
                            && command.contains("CODETERM_API_URL")
                            && command.contains("CODETERM_API_TOKEN")
                            && command.contains("CODETERM_SESSION_BINDING_NONCE")
                            && !command.contains("CODETERM_API_PORT")
                            && !command.contains("CODETERM_PANE_ID")
                            && !command.contains("/api/panes/")
                    })
                    .unwrap_or(false);
                current_marker && current_command
            })
        })
        .unwrap_or(false)
}

/// Returns JSON shaped like the legacy `ClaudeHooksStatus` struct so the
/// frontend SchemaRenderer's Badge mapper picks up `hooks_installed`
/// unchanged. New consumers can also look at the per-hook fields.
pub fn check_integration() -> Json {
    let Ok(path) = claude_settings_path() else {
        return serde_json::json!({
            "settings_exists": false,
            "hooks_installed": false,
            "notification_hook": false,
            "stop_hook": false,
            "precompact_hook": false,
            "session_start_hook": false,
            "target": "~/.claude/settings.json",
            "backup_exists": false,
            "proposed_changes": ["Notification", "Stop", "SessionStart"],
        });
    };
    if !path.exists() {
        return serde_json::json!({
            "settings_exists": false,
            "hooks_installed": false,
            "notification_hook": false,
            "stop_hook": false,
            "precompact_hook": false,
            "session_start_hook": false,
            "target": path,
            "backup_exists": backup_path(&path).exists(),
            "proposed_changes": ["Notification", "Stop", "SessionStart"],
        });
    }

    let Ok(content) = std::fs::read_to_string(&path) else {
        return serde_json::json!({ "settings_exists": true, "hooks_installed": false });
    };
    let Ok(json): Result<Json, _> = serde_json::from_str(&content) else {
        return serde_json::json!({ "settings_exists": true, "hooks_installed": false });
    };

    let notification_hook = json
        .pointer("/hooks/Notification")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let stop_hook = json
        .pointer("/hooks/Stop")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let precompact_hook = json
        .pointer("/hooks/PreCompact")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let session_start_hook = json
        .pointer("/hooks/SessionStart")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_session_start_hook))
        .unwrap_or(false);

    serde_json::json!({
        "settings_exists": true,
        "hooks_installed": notification_hook && stop_hook && session_start_hook,
        "notification_hook": notification_hook,
        "stop_hook": stop_hook,
        "precompact_hook": precompact_hook,
        "session_start_hook": session_start_hook,
        "target": path,
        "backup_exists": backup_path(&path).exists(),
        "proposed_changes": ["Notification", "Stop", "SessionStart"],
    })
}

fn hook_command(path: &str, body: &str, extra_headers: &str, stdin: bool) -> String {
    let payload = if stdin {
        "--data-binary @-".to_string()
    } else {
        format!("-d '{body}'")
    };
    format!(
        "if [ -n \"${{CODETERM_API_URL:-}}\" ] && [ -n \"${{CODETERM_API_TOKEN:-}}\" ] && [ -n \"${{CODETERM_TAB_ID:-}}\" ]; then curl -s -o /dev/null -X POST \"$CODETERM_API_URL{path}\" -H 'Content-Type: application/json' -H \"Authorization: Bearer $CODETERM_API_TOKEN\" {extra_headers} {payload} || true; fi"
    )
}

fn install_integration_at(
    path: &std::path::Path,
    options: &IntegrationInstallOptions,
) -> Result<(), String> {
    if !options.consent {
        return Err("provider integration installation requires explicit local-user consent".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let existed = path.exists();
    let original = if existed {
        std::fs::read_to_string(path).map_err(|e| e.to_string())?
    } else {
        "{}".into()
    };
    let mut json: Json = if existed {
        let content = original.clone();
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))?
    } else {
        serde_json::json!({})
    };

    let nonce_h = "-H \"X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}\"";
    let notify_cmd = hook_command(
        "/api/tabs/$CODETERM_TAB_ID/hook/permission",
        "{}",
        "",
        false,
    );
    let stop_cmd = hook_command("/api/tabs/$CODETERM_TAB_ID/hook/stop", "{}", "", false);
    let precompact_cmd = hook_command(
        "/api/mem/flush",
        "{\"tab\":\"'\"$CODETERM_TAB_ID\"'\",\"title\":\"pre-compact\"}",
        "",
        false,
    );
    let session_start_cmd = hook_command(
        "/api/tabs/$CODETERM_TAB_ID/hook/session_start",
        "",
        nonce_h,
        true,
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

    let hooks = json["hooks"]
        .as_object_mut()
        .ok_or("hooks is not an object")?;

    for key in &["Notification", "Stop", "PreCompact", "SessionStart"] {
        if hooks.get(*key).is_some_and(|value| !value.is_array()) {
            return Err(format!("hooks.{key} is not an array"));
        }
        if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
            arr.retain(|h| !is_codeterm_hook(h));
        }
    }

    if !hooks.contains_key("Notification") {
        hooks.insert("Notification".into(), serde_json::json!([]));
    }
    hooks["Notification"]
        .as_array_mut()
        .unwrap()
        .push(notif_hook);
    if !hooks.contains_key("Stop") {
        hooks.insert("Stop".into(), serde_json::json!([]));
    }
    hooks["Stop"].as_array_mut().unwrap().push(stop_hook);
    if options.precompact_memory_export {
        if !hooks.contains_key("PreCompact") {
            hooks.insert("PreCompact".into(), serde_json::json!([]));
        }
        hooks["PreCompact"]
            .as_array_mut()
            .unwrap()
            .push(precompact_hook);
    }
    if !hooks.contains_key("SessionStart") {
        hooks.insert("SessionStart".into(), serde_json::json!([]));
    }
    hooks["SessionStart"]
        .as_array_mut()
        .unwrap()
        .push(session_start_hook);

    let backup = backup_path(path);
    if !backup.exists() {
        std::fs::write(backup, if existed { original } else { String::new() })
            .map_err(|e| e.to_string())?;
    }
    write_json_atomic(path, &json)?;
    log::info!("[agent-claude] hooks installed at {:?}", path);
    Ok(())
}

pub fn install_integration(
    _api_port: u16,
    options: &IntegrationInstallOptions,
) -> Result<(), String> {
    install_integration_at(&claude_settings_path()?, options)
}

pub fn uninstall_integration() -> Result<(), String> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut json: Json = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings.json: {}", e))?;

    if let Some(hooks) = json.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        for key in &["Notification", "Stop", "PreCompact", "SessionStart"] {
            if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
                arr.retain(|h| !is_codeterm_hook(h));
            }
        }
    }

    write_json_atomic(&path, &json)?;
    log::info!("[agent-claude] hooks removed");
    Ok(())
}

fn restore_integration_at(path: &std::path::Path) -> Result<(), String> {
    let backup = backup_path(&path);
    let original = std::fs::read(&backup)
        .map_err(|error| format!("No recoverable integration backup at {}: {error}", backup.display()))?;
    if original.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|error| error.to_string())?;
        }
    } else {
        std::fs::write(&path, original).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn restore_integration() -> Result<(), String> {
    restore_integration_at(&claude_settings_path()?)
}

pub fn ensure_integration_on_startup(api_port: u16) {
    let _ = api_port;
    let _ = check_integration();
}

// ── Project rule (~/.claude/rules/codeterm.md) ──

fn extract_revision(content: &str) -> u32 {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("<!-- revision: ") {
            if let Some(end) = rest.find(" -->") {
                if let Ok(n) = rest[..end].parse::<u32>() {
                    return n;
                }
            }
        }
    }
    0
}

pub fn check_rule_status() -> Json {
    let Ok(path) = claude_rule_path() else {
        return serde_json::json!({
            "installed": false,
            "update_available": false,
            "revision": 0,
            "installed_revision": 0,
        });
    };
    let bundled_revision = extract_revision(CODETERM_RULE);
    if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let installed_revision = extract_revision(&content);
        serde_json::json!({
            "installed": true,
            "update_available": installed_revision < bundled_revision,
            "revision": bundled_revision,
            "installed_revision": installed_revision,
        })
    } else {
        serde_json::json!({
            "installed": false,
            "update_available": false,
            "revision": bundled_revision,
            "installed_revision": 0,
        })
    }
}

pub fn install_rule() -> Result<(), String> {
    let dest = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".claude")
        .join("rules");
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    std::fs::write(dest.join("codeterm.md"), CODETERM_RULE).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integration_options(precompact_memory_export: bool) -> IntegrationInstallOptions {
        IntegrationInstallOptions {
            consent: true,
            precompact_memory_export,
        }
    }

    #[test]
    fn extract_revision_reads_trailing_marker() {
        assert_eq!(extract_revision("# foo\n<!-- revision: 7 -->\n"), 7);
    }

    #[test]
    fn extract_revision_missing_returns_zero() {
        assert_eq!(extract_revision("# no marker here"), 0);
    }

    #[test]
    fn check_integration_without_file_reports_missing() {
        // We can't write to ~/.claude in tests, just check the shape when home doesn't exist or settings file is absent. The function gracefully falls back, returning settings_exists:false.
        let result = check_integration();
        assert!(result.get("settings_exists").is_some());
        assert!(result.get("hooks_installed").is_some());
    }

    #[test]
    fn current_marker_does_not_accept_the_stale_pane_contract() {
        let stale = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": "curl http://localhost:7686/api/panes/$CODETERM_PANE_ID/hook/session_start",
                "_source": CODETERM_HOOK_MARKER
            }]
        });
        let current = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": "curl \"$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start\" -H \"Authorization: Bearer $CODETERM_API_TOKEN\"",
                "_source": CODETERM_HOOK_MARKER
            }]
        });

        assert!(!is_current_codeterm_hook(&stale));
        assert!(is_current_codeterm_hook(&current));
    }

    #[test]
    fn session_start_nonce_must_be_in_the_current_hook_command() {
        let stale = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": "curl http://localhost:7686/api/tabs/$CODETERM_TAB_ID/hook/session_start",
                "_source": CODETERM_HOOK_MARKER
            }, {
                "type": "command",
                "command": "echo $CODETERM_SESSION_BINDING_NONCE",
                "_source": "other"
            }]
        });
        let current = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": "curl \"$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start\" -H \"Authorization: Bearer $CODETERM_API_TOKEN\" -H \"X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}\"",
                "_source": CODETERM_HOOK_MARKER
            }]
        });

        assert!(!is_current_session_start_hook(&stale));
        assert!(is_current_session_start_hook(&current));
    }

    #[test]
    fn integration_is_instance_neutral_and_rejects_missing_runtime_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        install_integration_at(&path, &integration_options(false)).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        install_integration_at(&path, &integration_options(false)).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();

        assert_eq!(first, second);
        assert!(first.contains("CODETERM_API_URL"));
        assert!(first.contains("CODETERM_API_TOKEN"));
        assert!(first.contains("CODETERM_TAB_ID"));
        assert!(first.contains("if [ -n"));
        assert!(!first.contains("CODETERM_API_PORT"));
        assert!(!first.contains("localhost:768"));
        assert!(!first.contains("> NUL"));
    }

    #[test]
    fn integration_requires_consent_and_restores_exact_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = "{\n  \"foreign\": { \"spacing\": true }\n}\n";
        std::fs::write(&path, original).unwrap();
        let denied = IntegrationInstallOptions {
            consent: false,
            precompact_memory_export: true,
        };
        assert!(install_integration_at(&path, &denied).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(!backup_path(&path).exists());

        install_integration_at(&path, &integration_options(false)).unwrap();
        assert_eq!(std::fs::read_to_string(backup_path(&path)).unwrap(), original);
        let standard: Json =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(standard.pointer("/hooks/PreCompact").is_none());
        install_integration_at(&path, &integration_options(true)).unwrap();
        assert_eq!(std::fs::read_to_string(backup_path(&path)).unwrap(), original);
        let installed: Json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(installed.pointer("/hooks/PreCompact").is_some());
        restore_integration_at(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}
