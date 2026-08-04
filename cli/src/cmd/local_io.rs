use std::io::Write;

use crate::args::misc::WorkspaceAction;
use crate::client::SocketClient;

pub(crate) fn workspace_create_cwd_hint() -> String {
    "hint: no --cwd — agents in this workspace inherit the daemon working directory; pass --cwd <project dir> for project work".to_string()
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

pub(crate) fn resolve_tab(client: &mut SocketClient, id: Option<String>) -> String {
    if let Some(id) = id {
        return id;
    }
    // 2. $CODETERM_TAB_ID env var (set by CodeTerm PTY for each shell)
    if let Ok(env_id) = std::env::var("CODETERM_TAB_ID") {
        if !env_id.is_empty() {
            return env_id;
        }
    }
    match client.call("tab.list", None) {
        Ok(value) => {
            if let Some(arr) = value.as_array() {
                let tab = arr
                    .iter()
                    .find(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                    .or_else(|| arr.first());
                if let Some(id) = tab.and_then(|p| p.get("id").and_then(|v| v.as_str())) {
                    return id.to_string();
                }
            }
            eprintln!("Error: no Tabs found");
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
            } else {
                eprintln!("{}", workspace_create_cwd_hint());
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
        WorkspaceAction::Close { id } => {
            let mut params = serde_json::json!({ "workspace_id": id });
            crate::cmd::plan::maybe_inject_caller_tab(&mut params);
            Some(client.call("workspace.close", Some(params)))
        }
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

#[allow(
    clippy::too_many_arguments,
    reason = "CLI wrapper preserves send text argument surface."
)]
pub(crate) fn handle_send_text(
    client: &mut SocketClient,
    json: bool,
    tab: Option<String>,
    text: Option<String>,
    from_file: Option<String>,
    from_pad: Option<String>,
    stdin: bool,
    actions: Option<String>,
    delay: Option<String>,
    at: Option<String>,
) {
    crate::cmd::send_text::handle_send_text(
        client, json, tab, text, from_file, from_pad, stdin, actions, delay, at,
    );
}

#[cfg(test)]
mod tests {
    use super::workspace_create_cwd_hint;

    #[test]
    fn workspace_cwd_hint_names_the_flag() {
        let hint = workspace_create_cwd_hint();
        assert!(hint.contains("--cwd"));
        assert!(hint.contains("daemon working directory"));
    }
}
