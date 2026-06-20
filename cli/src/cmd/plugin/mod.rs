//! CLI front-end for plugin management. The filesystem-level logic lives in the
//! shared `codeterm-plugins` crate (called in-process by both this CLI and the
//! daemon); this module only converts the clap action, prints the result, and
//! handles the one path the crate can't — the `plugin <id> <verb>` passthrough,
//! which routes over the daemon socket.

use crate::args::plugin::{PluginAction, PluginChannelAction};
use codeterm_plugins::{PluginAction as Action, PluginChannelAction as ChannelAction, PluginCtx};

fn plugin_ctx() -> PluginCtx {
    PluginCtx {
        data_root: crate::data_root(),
        version: std::env::var("CODETERM_VERSION")
            .ok()
            .filter(|s| !s.trim().is_empty()),
    }
}

pub fn handle_plugin(json: bool, action: PluginAction) {
    let ctx = plugin_ctx();
    let result = match action {
        PluginAction::Pass(parts) => {
            let r = pass_command(parts.clone());
            if let Err(e) = &r {
                codeterm_plugins::audit_failure(ctx, &Action::Pass(parts), e);
            }
            r
        }
        other => codeterm_plugins::run(ctx, convert_action(other)),
    };

    match result {
        Ok(value) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            } else {
                print_human(&value);
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

fn convert_action(action: PluginAction) -> Action {
    match action {
        PluginAction::List => Action::List,
        PluginAction::Install {
            id,
            from_dir,
            id_override,
            channel,
        } => Action::Install {
            id,
            from_dir,
            id_override,
            channel,
        },
        PluginAction::Update { id } => Action::Update { id },
        PluginAction::UpdateSystem { id } => Action::UpdateSystem { id },
        PluginAction::Remove {
            id,
            force,
            remove_data,
        } => Action::Remove {
            id,
            force,
            remove_data,
        },
        PluginAction::Channel { command } => Action::Channel(convert_channel(command)),
        PluginAction::Get { id } => Action::Get { id },
        PluginAction::Config {
            id,
            set,
            values_json,
            grant,
            revoke,
        } => Action::Config {
            id,
            set,
            values_json,
            grant,
            revoke,
        },
        PluginAction::Pass(parts) => Action::Pass(parts),
    }
}

fn convert_channel(action: PluginChannelAction) -> ChannelAction {
    match action {
        PluginChannelAction::Add { url, name } => ChannelAction::Add { url, name },
        PluginChannelAction::EnsureDefault => ChannelAction::EnsureDefault,
        PluginChannelAction::List => ChannelAction::List,
        PluginChannelAction::Refresh { name } => ChannelAction::Refresh { name },
        PluginChannelAction::Diff { name } => ChannelAction::Diff { name },
        PluginChannelAction::Remove { name, force } => ChannelAction::Remove { name, force },
    }
}

fn print_human(value: &serde_json::Value) {
    if let Some(arr) = value.get("plugins").and_then(|v| v.as_array()) {
        if arr.is_empty() {
            println!("No plugins installed.");
            return;
        }
        for p in arr {
            let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let version = p
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("");
            println!("{id}\t{version}\t{path}");
        }
        return;
    }
    if let Some(arr) = value.get("channels").and_then(|v| v.as_array()) {
        if arr.is_empty() {
            println!("No plugin channels registered.");
            return;
        }
        for c in arr {
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let url = c.get("url").and_then(|v| v.as_str()).unwrap_or("");
            println!("{name}\t{url}");
        }
        return;
    }
    if let Some(msg) = value.get("message").and_then(|v| v.as_str()) {
        println!("{msg}");
        return;
    }
    // Passthrough (`codeterm plugin <id> <verb>`) → the plugin's `{ result }`.
    if let Some(result) = value.get("result").and_then(|v| v.as_str()) {
        println!("{result}");
        return;
    }
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

/// `codeterm plugin <id> <verb> [args…]` → route `plugin:<id>.<verb>` over the
/// socket to the owning plugin's `onAgentCommand` (the inverse seam). The caller's
/// worker/pane id is sent as `sessionId` so the host can scope the verb to the
/// commands the plugin declared at spawn. A plugin-reported `{ error }` surfaces
/// as a non-zero exit.
fn pass_command(parts: Vec<String>) -> Result<serde_json::Value, String> {
    let mut it = parts.into_iter();
    let id = it
        .next()
        .ok_or("usage: codeterm plugin <id> <verb> [args…]")?;
    let verb = it
        .next()
        .ok_or("usage: codeterm plugin <id> <verb> [args…]")?;
    let args: Vec<String> = it.collect();

    let socket = crate::client::discover_socket()?;
    let mut client = crate::client::SocketClient::connect(&socket)?;
    let session_id = std::env::var("CODETERM_WORKER_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("CODETERM_PANE_ID").ok())
        .unwrap_or_default();
    let method = format!("plugin:{id}.{verb}");
    let params = serde_json::json!({ "args": args, "sessionId": session_id });
    let resp = client.call(&method, Some(params))?;
    if let Some(e) = resp.get("error").and_then(|v| v.as_str()) {
        return Err(e.to_string());
    }
    Ok(resp)
}
