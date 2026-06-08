//! Machine-readable dump of the clap command tree — the single source of truth
//! consumed by docs/CLI.md, the prompt doctor, and the chat-card templates.
//! Regenerate committed artifacts with `codeterm cli-manifest --write`.

use clap::{Command, CommandFactory};
use serde::Serialize;

use crate::args::cli::Cli;

#[derive(Serialize, PartialEq, Debug)]
pub struct ManifestArg {
    pub name: String,
    pub positional: bool,
    pub value_name: Option<String>,
    pub default: Option<String>,
    pub help: Option<String>,
    pub aliases: Vec<String>,
}

#[derive(Serialize, PartialEq, Debug)]
pub struct ManifestCommand {
    pub name: String,
    pub about: Option<String>,
    pub aliases: Vec<String>,
    pub args: Vec<ManifestArg>,
    pub subcommands: Vec<ManifestCommand>,
}

/// Build the full manifest from the live clap tree.
pub fn build() -> ManifestCommand {
    walk(&Cli::command())
}

fn walk(cmd: &Command) -> ManifestCommand {
    let mut args: Vec<ManifestArg> = cmd
        .get_arguments()
        .filter(|a| a.get_id() != "help" && a.get_id() != "version")
        .map(|a| ManifestArg {
            name: a.get_id().to_string(),
            positional: a.is_positional(),
            value_name: a
                .get_value_names()
                .and_then(|v| v.first())
                .map(|s| s.to_string()),
            default: a
                .get_default_values()
                .first()
                .map(|s| s.to_string_lossy().to_string()),
            help: a.get_help().map(|s| s.to_string()),
            aliases: a
                .get_visible_aliases()
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        })
        .collect();
    args.sort_by(|a, b| a.name.cmp(&b.name));

    let mut subcommands: Vec<ManifestCommand> = cmd.get_subcommands().map(walk).collect();
    subcommands.sort_by(|a, b| a.name.cmp(&b.name));

    let mut aliases: Vec<String> = cmd.get_visible_aliases().map(|s| s.to_string()).collect();
    aliases.sort();

    ManifestCommand {
        name: cmd.get_name().to_string(),
        about: cmd.get_about().map(|s| s.to_string()),
        aliases,
        args,
        subcommands,
    }
}

/// Canonical pretty JSON of the manifest (trailing newline, diff-stable).
pub fn to_json() -> String {
    let mut s = serde_json::to_string_pretty(&build()).expect("serialize manifest");
    s.push('\n');
    s
}

pub fn handle(write: bool) {
    if write {
        eprintln!("--write not implemented yet");
        std::process::exit(1);
    }
    print!("{}", to_json());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_has_core_commands() {
        let m = build();
        let names: Vec<&str> = m.subcommands.iter().map(|c| c.name.as_str()).collect();
        for expected in ["send-text", "agent", "mem", "pane", "cli-manifest"] {
            assert!(
                names.contains(&expected),
                "missing top-level command: {expected}"
            );
        }
    }
}
