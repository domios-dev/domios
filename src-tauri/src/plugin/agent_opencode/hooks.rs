//! OpenCode hook installer — owned by the agent-opencode plugin.
//!
//! OpenCode (sst/opencode) doesn't have Claude/Codex-style hook events
//! configured via JSON; it has a **plugin** system (Bun JS modules)
//! registered in `opencode.json` under the top-level `plugin` array.
//! Each plugin module exports a function returning a `Hooks` object
//! whose keys are lifecycle events (`tool.execute.before`,
//! `chat.message`, `event`, etc).
//!
//! Verified against `references/opencode/packages/plugin/src/index.ts`
//! (the `Hooks` interface) and `references/opencode/packages/core/src/global.ts`
//! (the XDG config-dir resolution used by OpenCode itself; rust's
//! `dirs::config_dir()` disagrees with xdg-basedir on macOS so we
//! reimplement the lookup).
//!
//! Strategy:
//!
//! 1. Drop a vendored JS plugin module at
//!    `<config_dir>/opencode/codeterm-hooks.js`. Fully owned by us —
//!    install = overwrite, uninstall = delete.
//! 2. Register the path in `<config_dir>/opencode/opencode.json` under
//!    the `plugin` array. Idempotent: dedupe by exact spec match before
//!    appending.
//!
//! The plugin itself shells out to `codeterm hook --source opencode
//! <event>` and translates OpenCode's tool.execute.before signature into
//! the JSON shape the CLI's pre_tool_use handler expects (Claude-flavored
//! `{tool_name, tool_input}`). On stdout we receive
//! `{hookSpecificOutput: {permissionDecision, updatedInput}}` and copy
//! `updatedInput.command` back into the tool args.

use crate::plugin::cli_path::codeterm_cli_path;
use serde_json::Value;
use std::path::PathBuf;

const PLUGIN_FILE_NAME: &str = "codeterm-hooks.js";
const PLUGIN_SPEC_PREFIX: &str = "file://";
const PLUGIN_BANNER_MARKER: &str = "@codeterm-hooks-token-v3";

#[derive(serde::Serialize)]
pub struct OpenCodeTokenHooksStatus {
    pub config_exists: bool,
    pub plugin_file_present: bool,
    pub plugin_registered: bool,
    pub hooks_installed: bool,
}

fn opencode_config_dir() -> Result<PathBuf, String> {
    // OpenCode uses xdg-basedir (npm), which on every OS prefers
    // $XDG_CONFIG_HOME and otherwise falls back to ~/.config (incl.
    // macOS — opencode doesn't use ~/Library/Application Support).
    // Windows xdg-basedir returns %APPDATA% as the config dir.
    if let Ok(override_dir) = std::env::var("OPENCODE_CONFIG_DIR") {
        if !override_dir.is_empty() {
            return Ok(PathBuf::from(override_dir).join("opencode"));
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join("opencode"));
        }
    }
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    if cfg!(windows) {
        // xdg-basedir on Windows resolves config to %LOCALAPPDATA%
        // (verified by reading the npm package source it ships).
        // %APPDATA% (Roaming) is what dirs::config_dir() returns on
        // Windows but disagrees with what OpenCode actually reads —
        // writing there would silently never reach the agent.
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            if !local.is_empty() {
                return Ok(PathBuf::from(local).join("opencode"));
            }
        }
        return Ok(home.join("AppData").join("Local").join("opencode"));
    }
    Ok(home.join(".config").join("opencode"))
}

fn plugin_file_path() -> Result<PathBuf, String> {
    Ok(opencode_config_dir()?.join(PLUGIN_FILE_NAME))
}

fn config_file_path() -> Result<PathBuf, String> {
    // OpenCode probes opencode.jsonc first then opencode.json. We always
    // write to .json (simpler, no comment preservation), only reading
    // .jsonc if no .json exists yet — that keeps us from clobbering a
    // user's commented config in unexpected ways.
    let dir = opencode_config_dir()?;
    let json = dir.join("opencode.json");
    if json.exists() {
        return Ok(json);
    }
    let jsonc = dir.join("opencode.jsonc");
    if jsonc.exists() {
        return Ok(jsonc);
    }
    Ok(json)
}

fn plugin_spec(plugin_path: &std::path::Path) -> String {
    // OpenCode accepts npm specs (`@scope/name`), absolute file paths,
    // and `file://` URLs. We use the file:// form because it round-trips
    // cleanly across OS path separators (single forward-slash component
    // joining) and is unambiguous about "this is a local path".
    let s = plugin_path.to_string_lossy().replace('\\', "/");
    format!("{}{}", PLUGIN_SPEC_PREFIX, s)
}

fn render_plugin_js() -> String {
    // Run the path through serde_json so quotes / backslashes / unicode
    // in the home dir survive as a valid JS string literal — protects
    // against pathological cases like `O"Hara` Windows usernames.
    let cli_lit = serde_json::to_string(&codeterm_cli_path()).unwrap_or_else(|_| "\"codeterm\"".into());
    format!(
        r#"// {marker}
// CodeTerm hook plugin for OpenCode. Auto-generated; safe to delete.
// Re-installed on every CodeTerm daemon start.

const CLI = {cli_lit};

async function run(event, payload) {{
  const proc = Bun.spawn([CLI, "hook", "--source", "opencode", event], {{
    stdin: "pipe",
    stdout: "pipe",
    stderr: "ignore",
  }});
  proc.stdin.write(JSON.stringify(payload));
  await proc.stdin.end();
  const out = await new Response(proc.stdout).text();
  await proc.exited;
  if (!out.trim()) return null;
  try {{ return JSON.parse(out); }} catch {{ return null; }}
}}

export const id = "codeterm-hooks";

export async function server(_input, _options) {{
  return {{
    "tool.execute.before": async (input, output) => {{
      const tool = (input?.tool || "").toLowerCase();
      if (tool !== "bash") return;
      const command = output?.args?.command;
      if (typeof command !== "string" || !command.trim()) return;
      const wire = {{
        tool_name: "Bash",
        tool_input: {{ command }},
      }};
      const res = await run("pre-tool-use", wire);
      const updated = res?.hookSpecificOutput?.updatedInput?.command;
      if (typeof updated === "string" && updated.trim()) {{
        output.args.command = updated;
      }}
    }},
  }};
}}
"#,
        marker = PLUGIN_BANNER_MARKER,
        cli_lit = cli_lit,
    )
}

fn read_config(path: &std::path::Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if content.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse opencode.json: {}", e))
}

fn write_config(path: &std::path::Path, json: &Value) -> Result<(), String> {
    let pretty = serde_json::to_string_pretty(json).map_err(|e| e.to_string())?;
    std::fs::write(path, pretty).map_err(|e| e.to_string())
}

fn plugin_array_contains_spec(json: &Value, spec: &str) -> bool {
    json.pointer("/plugin")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().any(|entry| {
                if let Some(s) = entry.as_str() {
                    return s == spec;
                }
                entry
                    .as_array()
                    .and_then(|inner| inner.first())
                    .and_then(|first| first.as_str())
                    .map(|s| s == spec)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

pub fn check_token_hooks() -> Result<OpenCodeTokenHooksStatus, String> {
    let plugin_path = plugin_file_path()?;
    let config_path = config_file_path()?;
    let config_exists = config_path.exists();
    let plugin_file_present = plugin_path.exists()
        && std::fs::read_to_string(&plugin_path)
            .map(|s| s.contains(PLUGIN_BANNER_MARKER))
            .unwrap_or(false);
    let plugin_registered = if config_exists {
        let json = read_config(&config_path)?;
        plugin_array_contains_spec(&json, &plugin_spec(&plugin_path))
    } else {
        false
    };
    Ok(OpenCodeTokenHooksStatus {
        config_exists,
        plugin_file_present,
        plugin_registered,
        hooks_installed: plugin_file_present && plugin_registered,
    })
}

pub fn install_token_hooks() -> Result<(), String> {
    let dir = opencode_config_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let plugin_path = plugin_file_path()?;
    std::fs::write(&plugin_path, render_plugin_js()).map_err(|e| e.to_string())?;

    let config_path = config_file_path()?;
    let mut json = read_config(&config_path)?;
    let spec = plugin_spec(&plugin_path);

    if json.get("plugin").is_none() {
        json["plugin"] = serde_json::json!([]);
    }
    let arr = json["plugin"]
        .as_array_mut()
        .ok_or("opencode.json `plugin` is not an array")?;

    // Drop any prior codeterm spec — recognized by suffix match on the
    // file name we control (handles users moving their config dir).
    arr.retain(|entry| {
        let s = match entry {
            Value::String(s) => Some(s.clone()),
            Value::Array(inner) => inner
                .first()
                .and_then(|v| v.as_str())
                .map(str::to_string),
            _ => None,
        };
        match s {
            Some(spec) => !spec.ends_with(PLUGIN_FILE_NAME),
            None => true,
        }
    });
    arr.push(Value::String(spec));

    write_config(&config_path, &json)?;
    log::info!("OpenCode token-tool plugin installed at {:?}", plugin_path);
    Ok(())
}

pub fn uninstall_token_hooks() -> Result<(), String> {
    let plugin_path = plugin_file_path()?;
    if plugin_path.exists() {
        let _ = std::fs::remove_file(&plugin_path);
    }
    let config_path = config_file_path()?;
    if config_path.exists() {
        let mut json = read_config(&config_path)?;
        if let Some(arr) = json.get_mut("plugin").and_then(|v| v.as_array_mut()) {
            arr.retain(|entry| {
                let s = match entry {
                    Value::String(s) => Some(s.clone()),
                    Value::Array(inner) => inner
                        .first()
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    _ => None,
                };
                match s {
                    Some(spec) => !spec.ends_with(PLUGIN_FILE_NAME),
                    None => true,
                }
            });
            // Drop the array entirely if we just emptied it.
            if arr.is_empty() {
                if let Some(obj) = json.as_object_mut() {
                    obj.remove("plugin");
                }
            }
        }
        write_config(&config_path, &json)?;
    }
    log::info!("OpenCode token-tool plugin removed");
    Ok(())
}

pub fn ensure_token_hooks_installed() {
    match check_token_hooks() {
        Ok(status) if status.hooks_installed => {
            log::debug!("[startup] opencode token hooks already installed");
        }
        Ok(_) | Err(_) => {
            if let Err(e) = install_token_hooks() {
                log::warn!("[startup] failed to auto-install opencode token hooks: {}", e);
            } else {
                log::info!("[startup] opencode token hooks auto-installed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    fn tempdir(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("codeterm-opencode-{}-{}-{}", name, pid, n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn with_xdg<F: FnOnce(&PathBuf) -> Result<(), String>>(name: &str, f: F) {
        let _g = home_lock().lock().unwrap_or_else(|p| p.into_inner());
        let dir = tempdir(name);
        let prev_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let prev_oc = std::env::var("OPENCODE_CONFIG_DIR").ok();
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        std::env::remove_var("OPENCODE_CONFIG_DIR");
        let res = f(&dir);
        if let Some(v) = prev_xdg {
            std::env::set_var("XDG_CONFIG_HOME", v);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        if let Some(v) = prev_oc {
            std::env::set_var("OPENCODE_CONFIG_DIR", v);
        }
        let _ = std::fs::remove_dir_all(&dir);
        res.unwrap();
    }

    #[test]
    fn install_writes_plugin_file_and_registers_in_config() {
        with_xdg("install", |dir| {
            install_token_hooks()?;
            let plugin = dir.join("opencode/codeterm-hooks.js");
            assert!(plugin.exists());
            let body = std::fs::read_to_string(&plugin).map_err(|e| e.to_string())?;
            assert!(body.contains(PLUGIN_BANNER_MARKER));
            assert!(body.contains("tool.execute.before"));
            let cfg = std::fs::read_to_string(dir.join("opencode/opencode.json"))
                .map_err(|e| e.to_string())?;
            let json: Value = serde_json::from_str(&cfg).map_err(|e| e.to_string())?;
            let arr = json
                .pointer("/plugin")
                .and_then(|v| v.as_array())
                .ok_or("plugin array missing")?;
            assert_eq!(arr.len(), 1);
            assert!(arr[0].as_str().unwrap().ends_with("codeterm-hooks.js"));
            assert!(arr[0].as_str().unwrap().starts_with("file://"));
            Ok(())
        });
    }

    #[test]
    fn check_reports_missing_when_nothing_installed() {
        with_xdg("missing", |_| {
            let st = check_token_hooks()?;
            assert!(!st.hooks_installed);
            assert!(!st.plugin_file_present);
            assert!(!st.plugin_registered);
            Ok(())
        });
    }

    #[test]
    fn install_is_idempotent_no_duplicate_plugin_entries() {
        with_xdg("idem", |dir| {
            install_token_hooks()?;
            install_token_hooks()?;
            install_token_hooks()?;
            let cfg = std::fs::read_to_string(dir.join("opencode/opencode.json"))
                .map_err(|e| e.to_string())?;
            let json: Value = serde_json::from_str(&cfg).map_err(|e| e.to_string())?;
            let arr = json
                .pointer("/plugin")
                .and_then(|v| v.as_array())
                .ok_or("plugin array missing")?;
            assert_eq!(arr.len(), 1, "should de-dupe");
            Ok(())
        });
    }

    #[test]
    fn install_preserves_other_plugin_entries() {
        with_xdg("preserve", |dir| {
            std::fs::create_dir_all(dir.join("opencode")).map_err(|e| e.to_string())?;
            let pre = serde_json::json!({
                "plugin": ["@scope/their-plugin"],
                "model": "anthropic/claude-sonnet"
            });
            std::fs::write(
                dir.join("opencode/opencode.json"),
                serde_json::to_string_pretty(&pre).unwrap(),
            )
            .map_err(|e| e.to_string())?;
            install_token_hooks()?;
            let cfg = std::fs::read_to_string(dir.join("opencode/opencode.json"))
                .map_err(|e| e.to_string())?;
            let json: Value = serde_json::from_str(&cfg).map_err(|e| e.to_string())?;
            let arr = json
                .pointer("/plugin")
                .and_then(|v| v.as_array())
                .ok_or("plugin array missing")?;
            assert_eq!(arr.len(), 2);
            assert!(arr.iter().any(|e| e.as_str() == Some("@scope/their-plugin")));
            assert!(arr.iter().any(|e| e
                .as_str()
                .map(|s| s.ends_with("codeterm-hooks.js"))
                .unwrap_or(false)));
            assert_eq!(json.get("model").and_then(Value::as_str), Some("anthropic/claude-sonnet"));
            Ok(())
        });
    }

    #[test]
    fn uninstall_removes_file_and_config_entry() {
        with_xdg("uninstall", |dir| {
            install_token_hooks()?;
            uninstall_token_hooks()?;
            assert!(!dir.join("opencode/codeterm-hooks.js").exists());
            let st = check_token_hooks()?;
            assert!(!st.hooks_installed);
            Ok(())
        });
    }

    #[test]
    fn opencode_config_dir_overrides() {
        let _g = home_lock().lock().unwrap_or_else(|p| p.into_inner());
        let prev_oc = std::env::var("OPENCODE_CONFIG_DIR").ok();
        let prev_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("OPENCODE_CONFIG_DIR", "/tmp/oc-override");
        let dir = opencode_config_dir().unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/oc-override/opencode"));
        std::env::remove_var("OPENCODE_CONFIG_DIR");
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-test");
        let dir = opencode_config_dir().unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/xdg-test/opencode"));
        if let Some(v) = prev_oc {
            std::env::set_var("OPENCODE_CONFIG_DIR", v);
        }
        if let Some(v) = prev_xdg {
            std::env::set_var("XDG_CONFIG_HOME", v);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn plugin_array_contains_spec_handles_string_and_tuple() {
        let json = serde_json::json!({
            "plugin": [
                "file:///a.js",
                ["file:///b.js", {"opt": 1}],
                "@scope/c"
            ]
        });
        assert!(plugin_array_contains_spec(&json, "file:///a.js"));
        assert!(plugin_array_contains_spec(&json, "file:///b.js"));
        assert!(plugin_array_contains_spec(&json, "@scope/c"));
        assert!(!plugin_array_contains_spec(&json, "file:///nope.js"));
    }
}
