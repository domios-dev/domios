//! CodeTerm integration with Claude Code — hooks installed into
//! `~/.claude/settings.json` so Claude POSTs back to CodeTerm on
//! permission prompts, task completion, and session start. Also owns the
//! project-rule (`~/.claude/rules/codeterm.md`) install/check logic.
//!
//! Everything here was previously in `commands/hooks.rs` and
//! `commands/agent.rs` with hardcoded `claude_*` names. It moved into
//! the Claude provider as part of the abstraction pass so adding a new
//! provider doesn't require touching `commands/`.

use super::hook_ownership::{
    entries_carry_marker, entry_carries_marker, entry_is_legacy_pane_codeterm_hook,
    is_codeterm_hook, marker_header_arg, CODETERM_HOOK_MARKER,
};
use crate::prompts::CODETERM_RULE;
use crate::provider::{HookEventBinding, HookIngestionRoute, IntegrationInstallOptions};
use serde_json::Value as Json;

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
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("settings.json")
    ))
}

fn backup_state_path(path: &std::path::Path) -> std::path::PathBuf {
    path.with_file_name(format!(
        "{}.codeterm-backup-state",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("settings.json")
    ))
}

fn write_json_atomic(path: &std::path::Path, json: &Json) -> Result<(), String> {
    let pretty = serde_json::to_string_pretty(json).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.codeterm-tmp");
    std::fs::write(&temporary, pretty).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

/// True only when the hook matches the CURRENT callback contract. Identity is
/// the command: Claude's serializer drops `_source`, so requiring it here once
/// reported a working hook as missing.
fn is_current_codeterm_hook(hook_entry: &Json) -> bool {
    entry_has_command(hook_entry, |command| {
        command.contains("CODETERM_TAB_ID")
            && command.contains("CODETERM_API_URL")
            && command.contains("CODETERM_API_TOKEN")
            && is_current_hook_command(command)
            && !command.contains("CODETERM_API_PORT")
            && !command.contains("CODETERM_PANE_ID")
    })
}

fn is_current_session_start_hook(hook_entry: &Json) -> bool {
    entry_has_command(hook_entry, |command| {
        command.contains("CODETERM_SESSION_BINDING_NONCE")
            && command.contains("CODETERM_TAB_ID")
            && command.contains("CODETERM_API_URL")
            && command.contains("CODETERM_API_TOKEN")
            && is_current_hook_command(command)
            && !command.contains("CODETERM_API_PORT")
            && !command.contains("CODETERM_PANE_ID")
    })
}

fn is_current_stop_hook(hook_entry: &Json) -> bool {
    is_current_codeterm_hook(hook_entry)
        && entry_has_command(hook_entry, |command| {
            command.contains("CODETERM_SESSION_BINDING_NONCE")
                && command.contains("--data-binary \"@-\"")
        })
}

fn is_current_hook_command(command: &str) -> bool {
    let executable = if cfg!(windows) { "curl.exe " } else { "curl " };
    command.starts_with(executable)
        && command.contains(" --request POST ")
        && command.contains(" --data-binary ")
        && !command.contains("if [")
        && !command.contains("|| true")
}

fn entry_has_command(hook_entry: &Json, matches: impl Fn(&str) -> bool) -> bool {
    hook_entry
        .get("hooks")
        .and_then(|hooks| hooks.as_array())
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(|command| command.as_str())
                    .is_some_and(&matches)
            })
        })
}

/// Every owned entry across the roster carries the durable command marker.
/// False means the machine still runs pre-marker hooks that the consented
/// reconciliation migrates once; it never means the hooks are broken.
fn owned_hooks_carry_marker(json: &Json) -> bool {
    hook_event_roster().iter().all(|binding| {
        json.pointer(&format!("/hooks/{}", binding.event))
            .and_then(|value| value.as_array())
            .map(|entries| entries_carry_marker(entries.as_slice()))
            .unwrap_or(true)
    })
}

fn integration_owned(json: &Json) -> bool {
    json.pointer("/hooks/PreCompact")
        .and_then(Json::as_array)
        .is_some_and(|entries| entries.iter().any(entry_carries_marker))
        || hook_event_roster().iter().any(|binding| {
            json.pointer(&format!("/hooks/{}", binding.event))
                .and_then(Json::as_array)
                .is_some_and(|entries| {
                    entries.iter().any(|entry| {
                        entry_carries_marker(entry) || entry_is_legacy_pane_codeterm_hook(entry)
                    })
                })
        })
}

fn remove_retired_precompact_commands(entries: &mut Vec<Json>) {
    entries.retain_mut(|entry| {
        let Some(commands) = entry.get_mut("hooks").and_then(Json::as_array_mut) else {
            return true;
        };
        let previous = commands.len();
        commands.retain(|command| !is_codeterm_hook(&serde_json::json!({"hooks":[command]})));
        commands.len() == previous || !commands.is_empty()
    });
}

fn owned_precompact_hook(json: &Json) -> bool {
    json.pointer("/hooks/PreCompact")
        .and_then(Json::as_array)
        .is_some_and(|entries| entries.iter().any(is_codeterm_hook))
}

/// Returns JSON shaped like the legacy `ClaudeHooksStatus` struct so the
/// frontend SchemaRenderer's Badge mapper picks up `hooks_installed`
/// unchanged. New consumers can also look at the per-hook fields.
pub fn check_integration() -> Json {
    let Ok(path) = claude_settings_path() else {
        return serde_json::json!({
            "settings_exists": false,
            "hooks_installed": false,
            "integration_owned": false,
            "notification_hook": false,
            "permission_request_hook": false,
            "stop_hook": false,

            "session_start_hook": false,
            "target": "~/.claude/settings.json",
            "backup_exists": false,
            "proposed_changes": ["PermissionRequest", "Notification", "Stop", "SessionStart"],
        });
    };
    check_integration_at(&path)
}

pub(crate) fn check_integration_at(path: &std::path::Path) -> Json {
    if !path.exists() {
        return serde_json::json!({
            "settings_exists": false,
            "hooks_installed": false,
            "integration_owned": false,
            "notification_hook": false,
            "permission_request_hook": false,
            "stop_hook": false,

            "session_start_hook": false,
            "target": path,
            "backup_exists": backup_path(path).exists(),
            "proposed_changes": ["PermissionRequest", "Notification", "Stop", "SessionStart"],
        });
    }

    let Ok(content) = std::fs::read_to_string(path) else {
        return serde_json::json!({
            "settings_exists": true,
            "hooks_installed": false,
            "integration_state": "unreadable",
            "target": path,
        });
    };
    let Ok(json): Result<Json, _> = serde_json::from_str(&content) else {
        return serde_json::json!({
            "settings_exists": true,
            "hooks_installed": false,
            "integration_state": "unreadable",
            "target": path,
        });
    };

    let notification_hook = json
        .pointer("/hooks/Notification")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let permission_request_hook = json
        .pointer("/hooks/PermissionRequest")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_codeterm_hook))
        .unwrap_or(false);
    let stop_hook = json
        .pointer("/hooks/Stop")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_stop_hook))
        .unwrap_or(false);
    let precompact_hook = owned_precompact_hook(&json);
    let session_start_hook = json
        .pointer("/hooks/SessionStart")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_current_session_start_hook))
        .unwrap_or(false);

    let hooks_installed =
        notification_hook && permission_request_hook && stop_hook && session_start_hook;

    serde_json::json!({
        "settings_exists": true,
        "hooks_installed": hooks_installed,
        "integration_owned": integration_owned(&json),
        "hook_contract_current": hooks_installed && owned_hooks_carry_marker(&json) && !precompact_hook,
        "notification_hook": notification_hook,
        "permission_request_hook": permission_request_hook,
        "stop_hook": stop_hook,

        "session_start_hook": session_start_hook,
        "target": path,
        "backup_exists": backup_path(path).exists(),
        "proposed_changes": ["PermissionRequest", "Notification", "Stop", "SessionStart"],
    })
}

/// Claude's declared hook event roster — its installed events as data.
/// Install/uninstall and core route registration all consume this declaration.
pub fn hook_event_roster() -> Vec<HookEventBinding> {
    vec![
        // The immediate per-tool signal. `Notification` is the same route's
        // late, optionless fallback and stays declared alongside it.
        HookEventBinding {
            event: "PermissionRequest".into(),
            route: HookIngestionRoute::Permission,
            verdict: None,
        },
        HookEventBinding {
            event: "Notification".into(),
            route: HookIngestionRoute::Permission,
            verdict: None,
        },
        HookEventBinding {
            event: "Stop".into(),
            route: HookIngestionRoute::Stop,
            verdict: None,
        },
        HookEventBinding {
            event: "SessionStart".into(),
            route: HookIngestionRoute::SessionStart,
            verdict: None,
        },
    ]
}

fn hook_command(path: &str, body: &str, extra_headers: &str, stdin: bool) -> String {
    let executable = if cfg!(windows) { "curl.exe" } else { "curl" };
    let payload = if stdin {
        "--data-binary \"@-\"".to_string()
    } else {
        format!("--data-binary \"{body}\"")
    };
    let marker = marker_header_arg();
    format!(
        "{executable} --silent --request POST \"$CODETERM_API_URL{path}\" --header \"Content-Type: application/json\" --header \"Authorization: Bearer $CODETERM_API_TOKEN\" {marker} {extra_headers} {payload}"
    )
}

/// The settings.json hook entry a declared binding installs — the handler side
/// of the roster. Every `HookIngestionRoute` variant must be covered here.
/// Keyed on the BINDING, not the route: two events may share a route and still
/// need different matchers (`PermissionRequest` fires per tool, `Notification`
/// only on the vendor's `permission_prompt`). `None` = this provider declares
/// no settings.json shape for that route and installs nothing.
fn hook_entry(binding: &HookEventBinding) -> Option<Json> {
    let nonce_h = "--header \"X-CodeTerm-Binding-Nonce: $CODETERM_SESSION_BINDING_NONCE\"";
    // Forward the vendor's stdin payload; an empty body throws away the
    // tool_name / tool_input / permission_suggestions it already sent.
    let permission_command = hook_command(
        "/api/tabs/$CODETERM_TAB_ID/hook/permission",
        "",
        nonce_h,
        true,
    );
    Some(match binding.route {
        HookIngestionRoute::Permission => serde_json::json!({
            "matcher": if binding.event == "Notification" { "permission_prompt" } else { "*" },
            "hooks": [{
                "type": "command",
                "command": permission_command,
                "timeout": 5000,
                "_source": CODETERM_HOOK_MARKER
            }]
        }),
        HookIngestionRoute::Stop => serde_json::json!({
            "matcher": "",
            "hooks": [{
                "type": "command",
                "command": hook_command("/api/tabs/$CODETERM_TAB_ID/hook/stop", "", nonce_h, true),
                "timeout": 5000,
                "_source": CODETERM_HOOK_MARKER
            }]
        }),
        HookIngestionRoute::SessionStart => serde_json::json!({
            "matcher": "",
            "hooks": [{
                "type": "command",
                "command": hook_command(
                    "/api/tabs/$CODETERM_TAB_ID/hook/session_start", "", nonce_h, true),
                "timeout": 5000,
                "_source": CODETERM_HOOK_MARKER
            }]
        }),
        // Claude declares no session-state event, so its roster never reaches
        // this arm; the match stays exhaustive so a new variant fails to build.
        HookIngestionRoute::SessionState => return None,
    })
}

pub fn install_integration_at(
    path: &std::path::Path,
    options: &IntegrationInstallOptions,
) -> Result<(), String> {
    if !options.consent {
        return Err(
            "provider integration installation requires explicit local-user consent".into(),
        );
    }
    let existed = path.exists();
    let original = if existed {
        std::fs::read_to_string(path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let mut json: Json = if existed {
        serde_json::from_str(&original)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))?
    } else {
        serde_json::json!({})
    };
    if !json.is_object() {
        return Err("settings.json must contain a JSON object".into());
    }

    if json.get("hooks").is_none() {
        json["hooks"] = serde_json::json!({});
    }

    let hooks = json["hooks"]
        .as_object_mut()
        .ok_or("hooks is not an object")?;

    let roster = hook_event_roster();
    for event in roster
        .iter()
        .map(|binding| binding.event.as_str())
        .chain(std::iter::once("PreCompact"))
    {
        if hooks.get(event).is_some_and(|value| !value.is_array()) {
            return Err(format!("hooks.{event} is not an array"));
        }
        if let Some(arr) = hooks.get_mut(event).and_then(|v| v.as_array_mut()) {
            if event == "PreCompact" {
                remove_retired_precompact_commands(arr);
            } else {
                arr.retain(|h| !is_codeterm_hook(h));
            }
        }
    }

    for binding in &roster {
        let Some(entry) = hook_entry(binding) else {
            continue;
        };
        hooks
            .entry(binding.event.clone())
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("hooks.{} is not an array", binding.event))?
            .push(entry);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let backup = backup_path(path);
    let backup_state = backup_state_path(path);
    match (backup.exists(), backup_state.exists()) {
        (false, false) => {
            std::fs::write(&backup, original).map_err(|e| e.to_string())?;
            std::fs::write(&backup_state, if existed { "present" } else { "absent" })
                .map_err(|e| e.to_string())?;
        }
        (true, true) => {}
        _ => return Err("integration backup state is incomplete".into()),
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

pub fn uninstall_integration_at(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut json: Json = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings.json: {}", e))?;
    let root = json
        .as_object_mut()
        .ok_or("settings.json must contain a JSON object")?;
    let Some(hooks_value) = root.get_mut("hooks") else {
        return Ok(());
    };
    let hooks = hooks_value
        .as_object_mut()
        .ok_or("hooks is not an object")?;
    let roster = hook_event_roster();
    for event in roster
        .iter()
        .map(|binding| binding.event.as_str())
        .chain(std::iter::once("PreCompact"))
    {
        if hooks.get(event).is_some_and(|value| !value.is_array()) {
            return Err(format!("hooks.{event} is not an array"));
        }
    }
    for event in roster
        .iter()
        .map(|binding| binding.event.as_str())
        .chain(std::iter::once("PreCompact"))
    {
        if let Some(arr) = hooks.get_mut(event).and_then(|value| value.as_array_mut()) {
            if event == "PreCompact" {
                remove_retired_precompact_commands(arr);
            } else {
                arr.retain(|hook| !is_codeterm_hook(hook));
            }
        }
    }

    write_json_atomic(path, &json)?;
    log::info!("[agent-claude] hooks removed");
    Ok(())
}

pub fn uninstall_integration() -> Result<(), String> {
    uninstall_integration_at(&claude_settings_path()?)
}

pub(crate) fn restore_integration_at(path: &std::path::Path) -> Result<(), String> {
    let backup = backup_path(&path);
    let original = std::fs::read(&backup).map_err(|error| {
        format!(
            "No recoverable integration backup at {}: {error}",
            backup.display()
        )
    })?;
    let was_absent = std::fs::read_to_string(backup_state_path(path))
        .map(|state| state == "absent")
        .unwrap_or_else(|_| original.is_empty());
    if was_absent {
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
    super::schemas::install_bridge_rule(CODETERM_RULE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integration_options() -> IntegrationInstallOptions {
        IntegrationInstallOptions {
            consent: true,
            repair: false,
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
                "command": format!(
                    "{} --silent --request POST \"$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start\" --header \"Content-Type: application/json\" --header \"Authorization: Bearer $CODETERM_API_TOKEN\" {} --header \"X-CodeTerm-Binding-Nonce: $CODETERM_SESSION_BINDING_NONCE\" --data-binary \"@-\"",
                    if cfg!(windows) { "curl.exe" } else { "curl" },
                    marker_header_arg()
                )
            }]
        });

        assert!(!is_current_codeterm_hook(&stale));
        assert!(is_current_codeterm_hook(&current));
    }

    #[test]
    fn legacy_pane_hook_is_owned_for_startup_reconciliation() {
        const LEGACY_SESSION_START: &str = r#"curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/session_start -H 'Content-Type: application/json' -H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" -d '{}' > /dev/null 2>&1 || true"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "hooks": {
                    "SessionStart": [{
                        "matcher": "",
                        "hooks": [{
                            "type": "command",
                            "command": LEGACY_SESSION_START
                        }]
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let status = check_integration_at(&path);
        assert_eq!(status["integration_owned"], true);
        assert_eq!(status["hooks_installed"], false);
        assert_eq!(status["hook_contract_current"], false);
    }

    #[test]
    fn session_start_hook_currentness_requires_binding_and_portable_command() {
        let stale = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": "if [ -n \"$CODETERM_API_URL\" ]; then curl -s -X POST \"$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start\" -H \"Authorization: Bearer $CODETERM_API_TOKEN\" -H \"X-CodeTerm-Binding-Nonce: $CODETERM_SESSION_BINDING_NONCE\" --data-binary @- || true; fi",
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
                "command": format!(
                    "{} --silent --request POST \"$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start\" --header \"Authorization: Bearer $CODETERM_API_TOKEN\" --header \"X-CodeTerm-Binding-Nonce: $CODETERM_SESSION_BINDING_NONCE\" --data-binary \"@-\"",
                    if cfg!(windows) { "curl.exe" } else { "curl" }
                ),
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
        install_integration_at(&path, &integration_options()).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        install_integration_at(&path, &integration_options()).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();

        assert_eq!(first, second);
        assert!(first.contains("CODETERM_API_URL"));
        assert!(first.contains("CODETERM_API_TOKEN"));
        assert!(first.contains("CODETERM_TAB_ID"));
        assert!(!first.contains("if ["));
        assert!(!first.contains("|| true"));
        assert!(first.contains("X-CodeTerm-Hook: codeterm-hook-v3"));
        assert!(!first.contains("CODETERM_API_PORT"));
        assert!(!first.contains("localhost:768"));
        assert!(!first.contains("> NUL"));
    }

    #[cfg(windows)]
    fn parse_generated_hook_command(command: &str) {
        let mut bash_candidates = Vec::new();
        for root in [
            std::env::var_os("ProgramFiles"),
            std::env::var_os("ProgramFiles(x86)"),
        ]
        .into_iter()
        .flatten()
        {
            let git = std::path::PathBuf::from(root).join("Git");
            bash_candidates.push(git.join("bin").join("bash.exe"));
            bash_candidates.push(git.join("usr").join("bin").join("bash.exe"));
        }
        bash_candidates.push(std::path::PathBuf::from(r"C:\msys64\usr\bin\bash.exe"));
        let bash = bash_candidates
            .into_iter()
            .find(|candidate| candidate.is_file())
            .expect("Git Bash is available through the repository toolchain");
        let bash_output = std::process::Command::new(bash)
            .args(["-n", "-c", command])
            .output()
            .expect("Git Bash starts for parsing generated hook commands");
        assert!(
            bash_output.status.success(),
            "Bash rejected generated hook command: {}",
            String::from_utf8_lossy(&bash_output.stderr)
        );

        let powershell_command = [
            "CODETERM_API_URL",
            "CODETERM_API_TOKEN",
            "CODETERM_TAB_ID",
            "CODETERM_SESSION_BINDING_NONCE",
        ]
        .iter()
        .fold(command.to_string(), |rewritten, name| {
            rewritten.replace(&format!("${name}"), &format!("$env:{name}"))
        });
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$tokens = $null; $errors = $null; $null = [System.Management.Automation.Language.Parser]::ParseInput($env:CODETERM_HOOK_COMMAND, [ref]$tokens, [ref]$errors); if ($errors.Count -gt 0) { exit 1 }",
            ])
            .env("CODETERM_HOOK_COMMAND", powershell_command)
            .output()
            .expect("PowerShell parser is available on Windows");
        assert!(
            output.status.success(),
            "PowerShell rejected generated hook command: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(not(windows))]
    fn parse_generated_hook_command(command: &str) {
        let output = std::process::Command::new("bash")
            .args(["-n", "-c", command])
            .output()
            .expect("Bash parser is available on POSIX");
        assert!(
            output.status.success(),
            "Bash rejected generated hook command: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn generated_hook_commands_parse_in_the_host_shell() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        install_integration_at(&path, &integration_options()).unwrap();
        let installed: Json =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let hooks = installed.get("hooks").and_then(Json::as_object).unwrap();

        for (event, entries) in hooks {
            let Some(entries) = entries.as_array() else {
                continue;
            };
            for hook in entries
                .iter()
                .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
            {
                let command = hook["command"].as_str().expect("command hook");
                assert!(is_current_hook_command(command), "stale hook: {command}");
                if event == "SessionStart" {
                    assert!(command.contains("/hook/session_start"));
                    assert!(command.contains("CODETERM_SESSION_BINDING_NONCE"));
                    assert!(command.contains("--data-binary \"@-\""));
                }
                parse_generated_hook_command(command);
            }
        }
    }

    #[test]
    fn hook_event_roster_declares_the_current_events() {
        use crate::provider::HookIngestionRoute;

        let declared: Vec<(String, HookIngestionRoute)> = hook_event_roster()
            .into_iter()
            .map(|binding| (binding.event, binding.route))
            .collect();
        assert_eq!(
            declared,
            vec![
                (
                    "PermissionRequest".to_string(),
                    HookIngestionRoute::Permission
                ),
                ("Notification".to_string(), HookIngestionRoute::Permission),
                ("Stop".to_string(), HookIngestionRoute::Stop),
                ("SessionStart".to_string(), HookIngestionRoute::SessionStart),
            ]
        );
    }

    #[test]
    fn install_writes_exactly_the_declared_hook_event_roster() {
        use std::collections::BTreeSet;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        install_integration_at(&path, &integration_options()).unwrap();
        let installed: Json =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let hooks = installed.get("hooks").and_then(|h| h.as_object()).unwrap();

        let declared: BTreeSet<String> = hook_event_roster().into_iter().map(|b| b.event).collect();
        let written: BTreeSet<String> = hooks.keys().cloned().collect();
        assert_eq!(written, declared, "installed events must equal the roster");
        for event in &declared {
            let entries = hooks[event].as_array().unwrap();
            assert_eq!(entries.len(), 1, "{event}: exactly one installed entry");
            assert!(is_codeterm_hook(&entries[0]), "{event}: codeterm-owned");
        }

        let without_precompact = dir.path().join("settings-no-precompact.json");
        install_integration_at(&without_precompact, &integration_options()).unwrap();
        let installed: Json =
            serde_json::from_str(&std::fs::read_to_string(&without_precompact).unwrap()).unwrap();
        let written: BTreeSet<String> = installed
            .get("hooks")
            .and_then(|h| h.as_object())
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let expected = declared;
        assert_eq!(written, expected);
    }

    #[test]
    fn integration_requires_consent_and_restores_exact_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = "{\n  \"foreign\": { \"spacing\": true }\n}\n";
        std::fs::write(&path, original).unwrap();
        let denied = IntegrationInstallOptions {
            consent: false,

            repair: false,
        };
        assert!(install_integration_at(&path, &denied).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(!backup_path(&path).exists());

        install_integration_at(&path, &integration_options()).unwrap();
        assert_eq!(
            std::fs::read_to_string(backup_path(&path)).unwrap(),
            original
        );
        let standard: Json =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(standard.pointer("/hooks/PreCompact").is_none());
        install_integration_at(&path, &integration_options()).unwrap();
        assert_eq!(
            std::fs::read_to_string(backup_path(&path)).unwrap(),
            original
        );
        let installed: Json =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(installed.pointer("/hooks/PreCompact").is_none());
        restore_integration_at(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
    #[test]
    fn permission_request_hook_forwards_the_payload_instead_of_an_empty_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        install_integration_at(&path, &integration_options()).unwrap();
        let installed: Json =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        for event in ["PermissionRequest", "Notification"] {
            let command = installed
                .pointer(&format!("/hooks/{event}/0/hooks/0/command"))
                .and_then(|c| c.as_str())
                .unwrap_or_else(|| panic!("{event} hook installed"));
            assert!(
                command.contains("--data-binary \"@-\""),
                "{event} must forward the stdin payload: {command}"
            );
            assert!(
                !command.contains("-d '{}'"),
                "{event} must not post an empty body: {command}"
            );
            assert!(
                command.contains("/hook/permission"),
                "{event} targets the existing permission route: {command}"
            );
            assert!(
                command.contains("CODETERM_SESSION_BINDING_NONCE"),
                "{event} must carry the binding nonce: {command}"
            );
        }
    }

    #[test]
    fn permission_request_and_notification_share_a_route_with_distinct_matchers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        install_integration_at(&path, &integration_options()).unwrap();
        let installed: Json =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        // Both declare HookIngestionRoute::Permission; keying hook_entry on the
        // binding is what lets them install different matchers.
        assert_eq!(
            installed.pointer("/hooks/PermissionRequest/0/matcher"),
            Some(&Json::from("*"))
        );
        assert_eq!(
            installed.pointer("/hooks/Notification/0/matcher"),
            Some(&Json::from("permission_prompt"))
        );
    }

    #[test]
    fn permission_request_uninstall_removes_the_declared_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        install_integration_at(&path, &integration_options()).unwrap();
        uninstall_integration_at(&path).unwrap();
        let cleaned: Json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            cleaned
                .pointer("/hooks/PermissionRequest")
                .and_then(|v| v.as_array())
                .map(|entries| entries.len()),
            Some(0),
            "uninstall iterates the roster, so a declared event is removed for free"
        );
    }

    #[test]
    fn permission_request_check_integration_requires_the_new_hook() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        install_integration_at(&path, &integration_options()).unwrap();
        let installed: Json =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let entry = installed.pointer("/hooks/PermissionRequest/0").unwrap();
        assert!(is_current_codeterm_hook(entry));
    }
}
