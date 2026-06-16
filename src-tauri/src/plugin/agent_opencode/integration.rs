use std::path::PathBuf;

use serde_json::Value as Json;

const CODETERM_OPENCODE_PLUGIN_MARKER: &str = "codeterm-opencode-session-binding-v2";
const CODETERM_OPENCODE_PLUGIN_FILE: &str = "codeterm-session-binding.js";

fn config_dir() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("OPENCODE_CONFIG_DIR") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    Ok(home.join(".config").join("opencode"))
}

fn config_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("opencode.json"))
}

fn plugin_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join(CODETERM_OPENCODE_PLUGIN_FILE))
}

fn plugin_entry() -> Result<String, String> {
    Ok(plugin_path()?.to_string_lossy().replace('\\', "/"))
}

fn plugin_entry_matches(entry: &Json, target: &str) -> bool {
    let value = match entry {
        Json::String(s) => Some(s.as_str()),
        Json::Array(items) => items.first().and_then(|v| v.as_str()),
        _ => None,
    };
    value
        .map(|s| s.replace('\\', "/") == target)
        .unwrap_or(false)
}

pub fn check_integration() -> Json {
    let plugin_file = plugin_path().ok();
    let plugin_exists = plugin_file.as_ref().map(|p| p.exists()).unwrap_or(false);
    let plugin_current = plugin_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.contains(CODETERM_OPENCODE_PLUGIN_MARKER))
        .unwrap_or(false);
    let Ok(path) = config_path() else {
        return serde_json::json!({
            "config_file_exists": false,
            "plugin_file_exists": plugin_exists,
            "plugin_file_current": plugin_current,
            "plugin_installed": false,
        });
    };
    if !path.exists() {
        return serde_json::json!({
            "config_file_exists": false,
            "plugin_file_exists": plugin_exists,
            "plugin_file_current": plugin_current,
            "plugin_installed": false,
        });
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
        return serde_json::json!({
            "config_file_exists": true,
            "plugin_file_exists": plugin_exists,
            "plugin_file_current": plugin_current,
            "plugin_installed": false,
        });
    };
    let Ok(json): Result<Json, _> = serde_json::from_str(&content) else {
        return serde_json::json!({
            "config_file_exists": true,
            "plugin_file_exists": plugin_exists,
            "plugin_file_current": plugin_current,
            "plugin_installed": false,
        });
    };
    let target = plugin_entry().unwrap_or_default();
    let plugin_installed = json
        .get("plugin")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|entry| plugin_entry_matches(entry, &target)))
        .unwrap_or(false);

    serde_json::json!({
        "config_file_exists": true,
        "plugin_file_exists": plugin_exists,
        "plugin_file_current": plugin_current,
        "plugin_installed": plugin_installed,
    })
}

pub fn install_integration(api_port: u16) -> Result<(), String> {
    let cfg_path = config_path()?;
    if let Some(parent) = cfg_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut json: Json = if cfg_path.exists() {
        let content = std::fs::read_to_string(&cfg_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse opencode.json: {}", e))?
    } else {
        serde_json::json!({
            "$schema": "https://opencode.ai/config.json"
        })
    };

    if !json.is_object() {
        return Err("opencode.json root must be an object".into());
    }
    if json.get("plugin").is_none() {
        json["plugin"] = serde_json::json!([]);
    }
    let target = plugin_entry()?;
    let plugins = json["plugin"]
        .as_array_mut()
        .ok_or("opencode.json plugin field must be an array")?;
    plugins.retain(|entry| !plugin_entry_matches(entry, &target));
    plugins.push(Json::String(target));

    let plugin = plugin_source(api_port);
    let plugin_path = plugin_path()?;
    std::fs::write(&plugin_path, plugin).map_err(|e| e.to_string())?;

    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&cfg_path, pretty).map_err(|e| e.to_string())?;
    log::info!(
        "[agent-opencode] session binding plugin installed at {:?}",
        plugin_path
    );
    Ok(())
}

pub fn uninstall_integration() -> Result<(), String> {
    let cfg_path = config_path()?;
    if cfg_path.exists() {
        let content = std::fs::read_to_string(&cfg_path).map_err(|e| e.to_string())?;
        let mut json: Json = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse opencode.json: {}", e))?;
        let target = plugin_entry()?;
        if let Some(plugins) = json.get_mut("plugin").and_then(|v| v.as_array_mut()) {
            plugins.retain(|entry| !plugin_entry_matches(entry, &target));
        }
        let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
        std::fs::write(&cfg_path, pretty).map_err(|e| e.to_string())?;
    }
    let plugin_path = plugin_path()?;
    if plugin_path.exists() {
        std::fs::remove_file(&plugin_path).map_err(|e| e.to_string())?;
    }
    log::info!("[agent-opencode] session binding plugin removed");
    Ok(())
}

pub fn ensure_integration_on_startup(api_port: u16) {
    let status = check_integration();
    let installed = status
        .get("plugin_installed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let plugin_exists = status
        .get("plugin_file_exists")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let plugin_current = status
        .get("plugin_file_current")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if installed && plugin_exists && plugin_current {
        log::debug!("[agent-opencode] session binding plugin already installed");
        return;
    }
    if let Err(e) = install_integration(api_port) {
        log::warn!(
            "[agent-opencode] failed to auto-install session binding plugin: {}",
            e
        );
    }
}

fn plugin_source(api_port: u16) -> String {
    format!(
        r#"// {marker}
const PROVIDER_ID = "opencode";
const FALLBACK_PORT = "{api_port}";

function endpoint() {{
  const pane = process.env.CODETERM_PANE_ID;
  if (!pane) return null;
  const base =
    process.env.CODETERM_API_URL ||
    `http://127.0.0.1:${{process.env.CODETERM_API_PORT || FALLBACK_PORT}}`;
  return `${{base}}/api/panes/${{encodeURIComponent(pane)}}/hook/session_start`;
}}

function normalizeSessionID(value) {{
  return typeof value === "string" && value.startsWith("ses_") ? value : null;
}}

function firstSessionID(...values) {{
  for (const value of values) {{
    const sessionID = normalizeSessionID(value);
    if (sessionID) return sessionID;
  }}
  return null;
}}

async function report(sessionID, source, directory) {{
  sessionID = normalizeSessionID(sessionID);
  if (!sessionID) return;
  const url = endpoint();
  if (!url) return;
  const headers = {{
    "content-type": "application/json",
    "x-codeterm-agent-provider": PROVIDER_ID,
  }};
  if (process.env.CODETERM_SESSION_BINDING_NONCE) {{
    headers["x-codeterm-binding-nonce"] = process.env.CODETERM_SESSION_BINDING_NONCE;
  }}
  if (process.env.CODETERM_API_TOKEN) {{
    headers.authorization = `Bearer ${{process.env.CODETERM_API_TOKEN}}`;
  }}
  await fetch(url, {{
    method: "POST",
    headers,
    body: JSON.stringify({{
      provider_id: PROVIDER_ID,
      session_id: sessionID,
      source,
      cwd: directory || process.cwd(),
    }}),
  }}).catch(() => {{}});
}}

function eventSessionID(event) {{
  const p = event?.properties || {{}};
  return firstSessionID(
    p.sessionID,
    p.session_id,
    p.session?.id,
    p.info?.sessionID,
    p.info?.session_id,
    p.message?.sessionID,
    p.message?.session_id,
    p.part?.sessionID,
    p.part?.session_id,
    event?.sessionID,
    event?.session_id,
    p.info?.id
  );
}}

function eventDirectory(event, fallback) {{
  return (
    event?.properties?.info?.directory ||
    event?.properties?.session?.directory ||
    event?.properties?.directory ||
    fallback
  );
}}

export default async function CodeTermSessionBindingPlugin({{ directory }}) {{
  return {{
    async event({{ event }}) {{
      if (
        event?.type === "session.created" ||
        event?.type === "session.updated" ||
        event?.type === "tui.session.select" ||
        event?.type === "message.updated" ||
        event?.type === "message.part.updated"
      ) {{
        await report(eventSessionID(event), event.type, eventDirectory(event, directory));
      }}
    }},
    async "chat.message"(input) {{
      await report(input?.sessionID, "chat.message", directory);
    }},
    async "tool.execute.before"(input) {{
      await report(input?.sessionID, "tool.execute.before", directory);
    }},
    async "shell.env"(input, output) {{
      await report(input?.sessionID, "shell.env", input?.cwd || directory);
      return output;
    }},
  }};
}}
"#,
        marker = CODETERM_OPENCODE_PLUGIN_MARKER
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_entry_matches_string_and_tuple() {
        let target = "C:/Users/User/.config/opencode/codeterm-session-binding.js";
        assert!(plugin_entry_matches(&serde_json::json!(target), target));
        assert!(plugin_entry_matches(
            &serde_json::json!([target.replace('/', "\\"), {"enabled": true}]),
            target
        ));
        assert!(!plugin_entry_matches(
            &serde_json::json!("other.js"),
            target
        ));
    }

    #[test]
    fn plugin_source_posts_session_start() {
        let source = plugin_source(7686);
        assert!(source.contains(CODETERM_OPENCODE_PLUGIN_MARKER));
        assert!(source.contains("/hook/session_start"));
        assert!(source.contains("session.created"));
        assert!(source.contains("session.updated"));
        assert!(source.contains("tui.session.select"));
        assert!(source.contains("message.updated"));
        assert!(source.contains("message.part.updated"));
        assert!(source.contains("chat.message"));
    }
}
