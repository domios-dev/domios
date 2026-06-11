mod types;
mod install;
mod update;
mod channel;
mod list;
mod state;
mod audit;
mod version;
mod util;

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

use install::*;
use update::*;
use channel::*;
use list::*;
use state::*;
use audit::*;
use version::*;
use util::*;
use types::*;
pub fn handle_plugin(json: bool, action: PluginAction) {
    let result = match action.clone() {
        PluginAction::List => list_installed(),
        PluginAction::Install { id, from_dir, id_override, channel } => {
            install_command(id, from_dir, id_override, channel)
        }
        PluginAction::Update { id } => update_command(id),
        PluginAction::UpdateSystem { id } => update_system_command(id),
        PluginAction::Remove { id, force } => remove_installed(&id, force),
        PluginAction::Channel { command } => channel_command(command),
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
            let version = p.get("version").and_then(|v| v.as_str()).unwrap_or("unknown");
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
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

