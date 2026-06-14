use serde_json::Value as Json;

const CODETERM_CODEX_HOOK_MARKER: &str = "codeterm-codex-session-binding-v1";
const CODETERM_CODEX_HOOK_SCRIPT: &str = "codeterm-session-start.ps1";

fn hooks_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".codex").join("hooks.json"))
}

fn windows_hook_script_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".codex").join(CODETERM_CODEX_HOOK_SCRIPT))
}

fn is_codeterm_hook(entry: &Json) -> bool {
    entry
        .get("hooks")
        .and_then(|hooks| hooks.as_array())
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("_source")
                    .and_then(|source| source.as_str())
                    .map(|source| source == CODETERM_CODEX_HOOK_MARKER)
                    .unwrap_or(false)
                    || hook
                        .get("command")
                        .and_then(|command| command.as_str())
                        .map(|command| {
                            command.contains("/hook/session_start")
                                && command.contains("X-CodeTerm-Agent-Provider: codex")
                        })
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn has_codeterm_windows_command(entry: &Json) -> bool {
    entry
        .get("hooks")
        .and_then(|hooks| hooks.as_array())
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("_source")
                    .and_then(|source| source.as_str())
                    .map(|source| source == CODETERM_CODEX_HOOK_MARKER)
                    .unwrap_or(false)
                    && hook
                        .get("commandWindows")
                        .and_then(|command| command.as_str())
                        .map(|command| command.contains(CODETERM_CODEX_HOOK_SCRIPT))
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

pub fn check_integration() -> Json {
    let Ok(path) = hooks_path() else {
        return serde_json::json!({
            "hooks_file_exists": false,
            "hooks_installed": false,
            "session_start_hook": false,
        });
    };
    if !path.exists() {
        return serde_json::json!({
            "hooks_file_exists": false,
            "hooks_installed": false,
            "session_start_hook": false,
        });
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
        return serde_json::json!({
            "hooks_file_exists": true,
            "hooks_installed": false,
            "session_start_hook": false,
        });
    };
    let Ok(json): Result<Json, _> = serde_json::from_str(&content) else {
        return serde_json::json!({
            "hooks_file_exists": true,
            "hooks_installed": false,
            "session_start_hook": false,
        });
    };
    let session_start_hook = json
        .pointer("/hooks/SessionStart")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(is_codeterm_hook))
        .unwrap_or(false);
    let windows_hook_ready = if cfg!(windows) {
        let script_exists = windows_hook_script_path()
            .map(|path| path.exists())
            .unwrap_or(false);
        let has_command = json
            .pointer("/hooks/SessionStart")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(has_codeterm_windows_command))
            .unwrap_or(false);
        script_exists && has_command
    } else {
        true
    };
    let installed = session_start_hook && windows_hook_ready;

    serde_json::json!({
        "hooks_file_exists": true,
        "hooks_installed": installed,
        "session_start_hook": installed,
    })
}

pub fn install_integration(api_port: u16) -> Result<(), String> {
    let path = hooks_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let script_path = windows_hook_script_path()?;
    std::fs::write(&script_path, windows_hook_script(api_port)).map_err(|e| e.to_string())?;

    let mut json: Json = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse hooks.json: {}", e))?
    } else {
        serde_json::json!({})
    };

    if json.get("hooks").is_none() {
        json["hooks"] = serde_json::json!({});
    }
    let hooks = json["hooks"]
        .as_object_mut()
        .ok_or("hooks is not an object")?;
    if let Some(arr) = hooks.get_mut("SessionStart").and_then(|v| v.as_array_mut()) {
        arr.retain(|entry| !is_codeterm_hook(entry));
    } else {
        hooks.insert("SessionStart".into(), serde_json::json!([]));
    }

    let null_dev = crate::shell::ShellSyntax::NULL_DEV;
    let command = format!(
        "curl -s -X POST http://localhost:${{CODETERM_API_PORT:-{api_port}}}/api/panes/$CODETERM_PANE_ID/hook/session_start -H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' -H \"Authorization: Bearer ${{CODETERM_API_TOKEN}}\" --data-binary @- > {null_dev} 2>&1 || true"
    );
    let command_windows = format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
        script_path.to_string_lossy()
    );
    let hook = serde_json::json!({
        "matcher": "startup|resume|clear|compact",
        "hooks": [{
            "type": "command",
            "command": command,
            "commandWindows": command_windows,
            "timeout": 5,
            "_source": CODETERM_CODEX_HOOK_MARKER
        }]
    });
    hooks["SessionStart"].as_array_mut().unwrap().push(hook);

    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    log::info!("[agent-codex] SessionStart hook installed at {:?}", path);
    Ok(())
}

pub fn uninstall_integration() -> Result<(), String> {
    let path = hooks_path()?;
    if !path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut json: Json =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse hooks.json: {}", e))?;
    if let Some(hooks) = json.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        if let Some(arr) = hooks.get_mut("SessionStart").and_then(|v| v.as_array_mut()) {
            arr.retain(|entry| !is_codeterm_hook(entry));
        }
    }
    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    if let Ok(script_path) = windows_hook_script_path() {
        let _ = std::fs::remove_file(script_path);
    }
    log::info!("[agent-codex] SessionStart hook removed");
    Ok(())
}

fn windows_hook_script(api_port: u16) -> String {
    format!(
        r#"# {marker}
$ErrorActionPreference = "SilentlyContinue"
$pane = $env:CODETERM_PANE_ID
if ([string]::IsNullOrWhiteSpace($pane)) {{ exit 0 }}
$port = if ($env:CODETERM_API_PORT) {{ $env:CODETERM_API_PORT }} else {{ "{api_port}" }}
$base = if ($env:CODETERM_API_URL) {{ $env:CODETERM_API_URL.TrimEnd("/") }} else {{ "http://127.0.0.1:$port" }}
$headers = @{{
  "Content-Type" = "application/json"
  "X-CodeTerm-Agent-Provider" = "codex"
}}
if ($env:CODETERM_API_TOKEN) {{
  $headers["Authorization"] = "Bearer $($env:CODETERM_API_TOKEN)"
}}
$body = [Console]::In.ReadToEnd()
Invoke-RestMethod -Method Post -Uri "$base/api/panes/$pane/hook/session_start" -Headers $headers -Body $body -TimeoutSec 5 | Out-Null
exit 0
"#,
        marker = CODETERM_CODEX_HOOK_MARKER
    )
}

pub fn ensure_integration_on_startup(api_port: u16) {
    let installed = check_integration()
        .get("hooks_installed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if installed {
        log::debug!("[agent-codex] SessionStart hook already installed");
        return;
    }
    if let Err(e) = install_integration(api_port) {
        log::warn!(
            "[agent-codex] failed to auto-install SessionStart hook: {}",
            e
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codeterm_hook_marker_matches_current_shape() {
        let entry = serde_json::json!({
            "matcher": "startup|resume|clear|compact",
            "hooks": [{
                "type": "command",
                "command": "curl http://localhost/api/panes/$CODETERM_PANE_ID/hook/session_start -H 'X-CodeTerm-Agent-Provider: codex'",
                "_source": CODETERM_CODEX_HOOK_MARKER
            }]
        });
        assert!(is_codeterm_hook(&entry));
        assert!(!has_codeterm_windows_command(&entry));
    }

    #[test]
    fn unrelated_hook_is_preserved() {
        let entry = serde_json::json!({
            "matcher": "startup",
            "hooks": [{"type": "command", "command": "echo user-hook"}]
        });
        assert!(!is_codeterm_hook(&entry));
    }
}
