use std::io::Write;

use crate::args::misc::WorkspaceAction;
use crate::client::SocketClient;

pub(crate) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC sequences: ESC [ ... final_byte
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc.is_ascii_alphabetic() || nc == '~' || nc == 'h' || nc == 'l' {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                // OSC sequence: ESC ] ... BEL or ST
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc == '\x07' {
                        break;
                    }
                    if nc == '\x1b' {
                        chars.next();
                        break;
                    }
                }
            }
        } else if c == '\r' {
            // Skip carriage return
        } else {
            out.push(c);
        }
    }
    out
}

pub(crate) fn workspace_reuse_notice(
    value: &serde_json::Value,
    requested_name: &str,
) -> Option<String> {
    if value.get("existing").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let ws_name = value.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let ws_id = value.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    Some(format!(
        "reused existing workspace {} '{}' (same cwd); requested name '{}' ignored",
        ws_id, ws_name, requested_name
    ))
}

pub(crate) fn write_workspace_create_reuse_output<W: Write, E: Write>(
    value: &serde_json::Value,
    requested_name: &str,
    json: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> std::io::Result<bool> {
    let Some(notice) = workspace_reuse_notice(value, requested_name) else {
        return Ok(false);
    };
    if json {
        writeln!(stdout, "{}", serde_json::to_string_pretty(value).unwrap())?;
    } else {
        writeln!(stderr, "{}", notice)?;
    }
    Ok(true)
}

pub(crate) fn resolve_pane(client: &mut SocketClient, id: Option<String>) -> String {
    // 1. Explicit --pane/--id arg
    if let Some(id) = id {
        return id;
    }
    // 2. $CODETERM_PANE_ID env var (set by CodeTerm PTY for each shell)
    if let Ok(env_id) = std::env::var("CODETERM_PANE_ID") {
        if !env_id.is_empty() {
            return env_id;
        }
    }
    // 3. Fallback: ask server for focused pane
    match client.call("pane.list", None) {
        Ok(value) => {
            if let Some(arr) = value.as_array() {
                let pane = arr
                    .iter()
                    .find(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                    .or_else(|| arr.first());
                if let Some(id) = pane.and_then(|p| p.get("id").and_then(|v| v.as_str())) {
                    return id.to_string();
                }
            }
            eprintln!("Error: no panes found");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

pub(crate) fn handle_workspace_action(
    client: &mut SocketClient,
    action: WorkspaceAction,
    json: bool,
) -> Option<Result<serde_json::Value, String>> {
    match action {
        WorkspaceAction::List => Some(client.call("workspace.list", None)),
        WorkspaceAction::Current => {
            let result = client.call("workspace.list", None);
            match result {
                Ok(value) => {
                    if let Some(arr) = value.as_array() {
                        let id = arr
                            .iter()
                            .find(|w| w.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                            .or_else(|| arr.first())
                            .and_then(|w| w.get("id").and_then(|v| v.as_str()));
                        if let Some(id) = id {
                            println!("{}", id);
                        }
                    }
                    None
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        WorkspaceAction::Create { name, cwd } => {
            let requested_name = name.clone();
            let mut params = serde_json::json!({ "name": name });
            if let Some(cwd) = cwd {
                params["cwd"] = serde_json::json!(cwd);
            }
            match client.call("workspace.create", Some(params)) {
                Ok(val) if workspace_reuse_notice(&val, &requested_name).is_some() => {
                    let _ = write_workspace_create_reuse_output(
                        &val,
                        &requested_name,
                        json,
                        &mut std::io::stdout(),
                        &mut std::io::stderr(),
                    );
                    None
                }
                result => Some(result),
            }
        }
        WorkspaceAction::Focus { id } => Some(client.call(
            "workspace.focus",
            Some(serde_json::json!({ "workspace_id": id })),
        )),
        WorkspaceAction::Rename { id, name } => Some(client.call(
            "workspace.rename",
            Some(serde_json::json!({ "workspace_id": id, "name": name })),
        )),
        WorkspaceAction::Close { id } => Some(client.call(
            "workspace.close",
            Some(serde_json::json!({ "workspace_id": id })),
        )),
        WorkspaceAction::MoveTab { tab_id, from, to } => Some(client.call(
            "workspace.move_tab",
            Some(serde_json::json!({
                "tab_id": tab_id,
                "from_workspace_id": from,
                "to_workspace_id": to
            })),
        )),
    }
}

pub(crate) fn handle_get_output(
    client: &mut SocketClient,
    pane: Option<String>,
    lines: u32,
    json: bool,
) {
    let pane_id = resolve_pane(client, pane);
    let max_bytes = (lines as u64) * 80;
    let result = client.call(
        "pane.get_output",
        Some(serde_json::json!({ "pane_id": pane_id, "max_bytes": max_bytes })),
    );
    match result {
        Ok(value) => {
            if let Some(output) = value.get("output").and_then(|v| v.as_str()) {
                if json {
                    println!("{}", serde_json::to_string_pretty(&value).unwrap());
                } else {
                    print!("{}", strip_ansi(output));
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

pub(crate) fn handle_send_text(
    client: &mut SocketClient,
    json: bool,
    pane: Option<String>,
    text: Option<String>,
    from_file: Option<String>,
    stdin: bool,
    submit: bool,
) {
    crate::cmd::send_text::handle_send_text(client, json, pane, text, from_file, stdin, submit);
}
