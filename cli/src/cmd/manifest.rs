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

/// Render the manifest as a nested markdown command reference.
pub fn to_markdown() -> String {
    let root = build();
    let mut out = String::new();
    for cmd in &root.subcommands {
        render_md(cmd, &[root.name.clone()], &mut out);
    }
    out
}

fn render_md(cmd: &ManifestCommand, path: &[String], out: &mut String) {
    let mut full = path.to_vec();
    full.push(cmd.name.clone());
    out.push_str(&format!("#### `{}`\n\n", full.join(" ")));
    if let Some(about) = &cmd.about {
        out.push_str(&format!("{about}\n\n"));
    }
    if !cmd.aliases.is_empty() {
        out.push_str(&format!("Aliases: {}\n\n", cmd.aliases.join(", ")));
    }
    for a in &cmd.args {
        let flag = if a.positional {
            format!("`<{}>`", a.name)
        } else {
            format!("`--{}`", a.name)
        };
        let help = a.help.as_deref().unwrap_or("");
        out.push_str(&format!("- {flag} — {help}\n"));
    }
    if !cmd.args.is_empty() {
        out.push('\n');
    }
    for sub in &cmd.subcommands {
        render_md(sub, &full, out);
    }
}

const MD_START: &str = "<!-- cli:auto-start -->";
const MD_END: &str = "<!-- cli:auto-end -->";

fn repo_path(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(rel)
}

pub fn handle(write: bool) {
    if !write {
        print!("{}", to_json());
        return;
    }
    let manifest = repo_path("docs/cli-manifest.json");
    std::fs::write(&manifest, to_json()).expect("write cli-manifest.json");
    println!("wrote {}", manifest.display());

    let cli_md = repo_path("docs/CLI.md");
    let md = std::fs::read_to_string(&cli_md).expect("read docs/CLI.md");
    let start = md.find(MD_START).expect("missing cli:auto-start marker") + MD_START.len();
    let end = md.find(MD_END).expect("missing cli:auto-end marker");
    let spliced = format!("{}\n{}\n{}", &md[..start], to_markdown(), &md[end..]);
    std::fs::write(&cli_md, spliced).expect("write docs/CLI.md");
    println!("wrote {}", cli_md.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/cli-manifest.json");

    #[test]
    fn cli_manifest_is_fresh() {
        let committed = std::fs::read_to_string(MANIFEST_PATH)
            .expect("docs/cli-manifest.json missing — run `codeterm cli-manifest --write`");
        assert_eq!(
            committed,
            to_json(),
            "docs/cli-manifest.json is stale — run `codeterm cli-manifest --write`"
        );
    }

    const CLI_MD_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/CLI.md");

    #[test]
    fn docs_cli_md_is_fresh() {
        let md = std::fs::read_to_string(CLI_MD_PATH).expect("docs/CLI.md missing");
        let start = md.find(MD_START).expect("missing cli:auto-start marker") + MD_START.len();
        let end = md.find(MD_END).expect("missing cli:auto-end marker");
        let block = &md[start..end];
        let expected = format!("\n{}\n", to_markdown());
        assert_eq!(
            block, expected,
            "docs/CLI.md auto-block stale — run `codeterm cli-manifest --write`"
        );
    }

    const PROMPT_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src-tauri/prompts/codeterm_cli.md"
    );

    fn resolve<'a>(root: &'a ManifestCommand, path: &[&str]) -> Option<&'a ManifestCommand> {
        let mut cur = root;
        for seg in path {
            cur = cur
                .subcommands
                .iter()
                .find(|c| c.name == *seg || c.aliases.iter().any(|a| a == seg))?;
        }
        Some(cur)
    }

    // Every `codeterm <subcmd...>` head referenced in the prompt must resolve in
    // the clap tree. Conservative: only flags a line whose command head matches
    // no real command at all.
    #[test]
    fn prompt_cli_commands_exist() {
        let root = build();
        let text = std::fs::read_to_string(PROMPT_PATH).expect("read codeterm_cli.md");
        let mut bad: Vec<String> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim().trim_start_matches('`').trim_start_matches("$ ");
            let Some(rest) = line.strip_prefix("codeterm ") else {
                continue;
            };
            // `codeterm --version` / `--help` etc. is the root + a global flag,
            // not a subcommand — valid, skip.
            if rest.trim_start().starts_with('-') {
                continue;
            }
            // Collect leading bare-word tokens as the candidate subcommand path.
            let mut path: Vec<&str> = Vec::new();
            for tok in rest.split_whitespace() {
                if tok.starts_with('-') || tok.starts_with('"') || tok.contains('=') {
                    break;
                }
                if tok.starts_with('<') || tok.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                    break; // argument placeholder (UPPERCASE / <...>)
                }
                path.push(tok);
            }
            // Trim back to the longest resolvable prefix.
            while !path.is_empty() && resolve(&root, &path).is_none() {
                path.pop();
            }
            if path.is_empty() {
                bad.push(format!("unrecognized: {line}"));
            }
        }
        assert!(
            bad.is_empty(),
            "prompt references unknown CLI commands:\n{}",
            bad.join("\n")
        );
    }

    #[test]
    fn manifest_has_core_commands() {
        let m = build();
        let names: Vec<&str> = m.subcommands.iter().map(|c| c.name.as_str()).collect();
        for expected in ["send", "agent", "mem", "pane", "cli-manifest"] {
            assert!(
                names.contains(&expected),
                "missing top-level command: {expected}"
            );
        }
    }
}
