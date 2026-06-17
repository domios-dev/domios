mod audit;
mod channel;
mod config;
mod install;
mod list;
mod state;
mod types;
mod update;
mod util;
mod version;

#[cfg(test)]
mod tests;

use crate::args::plugin::{PluginAction, PluginChannelAction};
use crate::data_root;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use audit::*;
use channel::*;
use install::*;
use list::*;
use state::*;
use types::*;
use update::*;
use util::*;
use version::*;
pub fn handle_plugin(json: bool, action: PluginAction) {
    let result = match action.clone() {
        PluginAction::List => list_installed(),
        PluginAction::Install {
            id,
            from_dir,
            id_override,
            channel,
        } => install_command(id, from_dir, id_override, channel),
        PluginAction::Update { id } => update_command(id),
        PluginAction::UpdateSystem { id } => update_system_command(id),
        PluginAction::Remove { id, force } => remove_installed(&id, force),
        PluginAction::Channel { command } => channel_command(command),
        PluginAction::Get { id } => config::get_command(&id),
        PluginAction::Config {
            id,
            set,
            values_json,
        } => config::config_command(&id, &set, values_json.as_deref()),
        PluginAction::Pass(parts) => pass_command(parts),
    };

    if let Err(e) = &result {
        let _ = audit_command_failure(&action, e);
    }

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
