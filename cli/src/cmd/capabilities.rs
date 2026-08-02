use clap::CommandFactory;
use serde_json::Value;

use crate::Cli;

#[derive(Debug, PartialEq, Eq)]
enum Projection {
    Cli(String),
    RpcOnly(Option<&'static str>),
}

fn command_exists(path: &str) -> bool {
    let mut command = Cli::command();
    for segment in path.split_whitespace() {
        let Some(next) = command
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == segment)
            .cloned()
        else {
            return false;
        };
        command = next;
    }
    true
}

fn direct_cli_override(method: &str) -> Option<&'static str> {
    match method {
        "system.identify" => Some("identify"),
        "system.capabilities" => Some("capabilities"),
        "notification.create" => Some("notify"),
        "notification.list" => Some("notifications"),
        "notification.clear" => Some("notifications clear"),
        "services.list" => Some("service list"),
        "services.create" => Some("service create"),
        "services.get" => Some("service get"),
        "services.update" => Some("service update"),
        "services.delete" => Some("service delete"),
        "services.regenerate_token" => Some("service regenerate-token"),
        _ => None,
    }
}

fn rpc_only_replacement(method: &str) -> Option<&'static str> {
    match method {
        "agent.children" | "agent.hierarchy" => Some("agent ledger"),
        "tab.send_text" => Some("send"),
        "tab.get_output" => Some("tab get-text"),
        _ => None,
    }
}

fn conventional_path(method: &str) -> String {
    method.replace('.', " ").replace('_', "-")
}

fn project(method: &str) -> Projection {
    if let Some(path) = direct_cli_override(method) {
        return if command_exists(path) {
            Projection::Cli(path.into())
        } else {
            Projection::RpcOnly(None)
        };
    }
    let candidate = conventional_path(method);
    if command_exists(&candidate) {
        Projection::Cli(candidate)
    } else {
        Projection::RpcOnly(rpc_only_replacement(method))
    }
}

pub fn render(value: &Value) -> String {
    let Some(methods) = value
        .get("methods")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
    else {
        return "No capabilities reported.\n".into();
    };
    let mut out = String::new();
    for method in methods.iter().filter_map(Value::as_str) {
        match project(method) {
            Projection::Cli(path) => {
                out.push_str(&format!("{method}  [CLI: `codeterm {path}`]\n"));
            }
            Projection::RpcOnly(Some(replacement)) => out.push_str(&format!(
                "{method}  [RPC only; use `codeterm {replacement}`]\n"
            )),
            Projection::RpcOnly(None) => {
                out.push_str(&format!("{method}  [RPC only/internal]\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{error::ErrorKind, Parser};
    use serde_json::json;

    fn advertised_methods() -> Vec<&'static str> {
        let source = include_str!("../../../src-tauri/src/socket/server/dispatch.rs");
        let block = source
            .split_once("let methods = vec![")
            .expect("capability declaration exists")
            .1
            .split_once("];")
            .expect("capability declaration closes")
            .0;
        block
            .lines()
            .filter_map(|line| line.trim().strip_prefix('"'))
            .filter_map(|line| line.split_once('"').map(|(method, _)| method))
            .collect()
    }

    fn command_path_parses(path: &str) -> bool {
        let mut args = vec!["codeterm"];
        args.extend(path.split_whitespace());
        args.push("--help");
        Cli::try_parse_from(args)
            .is_err_and(|error| error.kind() == ErrorKind::DisplayHelp)
    }

    #[test]
    fn capabilities_mark_rpc_only_and_name_cli_replacements() {
        let out = render(&json!({"methods": [
            "system.identify",
            "notification.create",
            "services.list",
            "mem.search",
            "agent.children",
            "tab.resize"
        ], "protocol": "json-rpc-2.0"}));
        assert!(out.contains("system.identify  [CLI: `codeterm identify`]"));
        assert!(out.contains("notification.create  [CLI: `codeterm notify`]"));
        assert!(out.contains("services.list  [CLI: `codeterm service list`]"));
        assert!(out.contains("mem.search  [CLI: `codeterm mem search`]"));
        assert!(out.contains("agent.children  [RPC only; use `codeterm agent ledger`]"));
        assert!(out.contains("tab.resize  [RPC only/internal]"));
    }

    #[test]
    fn every_advertised_capability_has_a_structurally_verified_projection() {
        let methods = advertised_methods();
        assert!(!methods.is_empty());
        for method in methods {
            match project(method) {
                Projection::Cli(path) => assert!(
                    command_path_parses(&path),
                    "{method} fabricated non-parsing CLI path: {path}"
                ),
                Projection::RpcOnly(Some(replacement)) => assert!(
                    command_path_parses(replacement),
                    "{method} names a non-parsing replacement: {replacement}"
                ),
                Projection::RpcOnly(None) => {}
            }
        }
    }
}
