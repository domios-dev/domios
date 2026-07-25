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
            name: if a.is_positional() {
                a.get_id().to_string()
            } else {
                a.get_long()
                    .map(|long| long.to_string())
                    .unwrap_or_else(|| a.get_id().to_string())
            },
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

    let mut subcommands: Vec<ManifestCommand> = cmd
        .get_subcommands()
        .filter(|c| c.get_name() != "help" && c.get_name() != "version" && !c.is_hide_set())
        .map(walk)
        .collect();
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

fn write_command_path_tree(command: &ManifestCommand, output: &mut String) {
    output.push('{');
    let mut first = true;
    for child in &command.subcommands {
        let mut branch = String::new();
        write_command_path_tree(child, &mut branch);
        for segment in std::iter::once(&child.name).chain(&child.aliases) {
            if !first {
                output.push(',');
            }
            first = false;
            output.push_str(&serde_json::to_string(segment).expect("serialize command segment"));
            output.push(':');
            output.push_str(&branch);
        }
    }
    output.push('}');
}

pub fn to_command_paths() -> String {
    let mut tree = String::new();
    write_command_path_tree(&build(), &mut tree);
    format!(
        "export interface CommandPathNode {{\n  readonly [segment: string]: CommandPathNode;\n}}\n\nexport const COMMAND_PATHS: CommandPathNode = {tree};\n"
    )
}

/// Render the manifest as a nested markdown command reference.
pub fn to_markdown() -> String {
    let root = build();
    let mut out = String::new();
    for cmd in &root.subcommands {
        render_md(cmd, std::slice::from_ref(&root.name), &mut out);
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
        if let Some(help) = a.help.as_deref().filter(|help| !help.is_empty()) {
            out.push_str(&format!("- {flag} — {help}\n"));
        } else {
            out.push_str(&format!("- {flag}\n"));
        }
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

pub fn handle(write: bool, markdown: bool) {
    if markdown {
        print!("{}", to_markdown());
        return;
    }
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

    let command_paths = repo_path("packages/chat-ui/src/codeterm/commandPaths.ts");
    std::fs::write(&command_paths, to_command_paths()).expect("write commandPaths.ts");
    println!("wrote {}", command_paths.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalize_newlines(s: &str) -> String {
        s.replace("\r\n", "\n")
    }

    const MANIFEST_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/cli-manifest.json");
    const CLI_MD_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/CLI.md");
    const COMMAND_PATHS_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../packages/chat-ui/src/codeterm/commandPaths.ts"
    );

    fn codeterm_exe() -> Option<std::path::PathBuf> {
        if let Ok(path) = std::env::var("CARGO_BIN_EXE_codeterm") {
            return Some(path.into());
        }
        let exe = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/codeterm");
        exe.is_file().then_some(exe)
    }

    fn run_codeterm(args: &[&str]) -> std::process::Output {
        if let Some(exe) = codeterm_exe() {
            return std::process::Command::new(&exe)
                .args(args)
                .output()
                .unwrap_or_else(|e| panic!("spawn {}: {e}", exe.display()));
        }
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let mut command =
            std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        command.args(["run", "--quiet", "--manifest-path"]);
        command.arg(manifest);
        command.args(["--bin", "codeterm", "--"]);
        command.args(args);
        command.output().expect("spawn cargo run --bin codeterm")
    }

    #[test]
    fn cli_manifest_is_fresh() {
        let committed = std::fs::read_to_string(MANIFEST_PATH)
            .expect("docs/cli-manifest.json missing — run `codeterm cli-manifest --write`");
        let output = run_codeterm(&["cli-manifest"]);
        assert!(
            output.status.success(),
            "codeterm cli-manifest failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let generated =
            String::from_utf8(output.stdout).expect("codeterm cli-manifest stdout is utf-8");
        assert_eq!(
            normalize_newlines(&committed),
            normalize_newlines(&generated),
            "docs/cli-manifest.json is stale — run `codeterm cli-manifest --write`"
        );
    }

    #[test]
    fn docs_cli_md_is_fresh() {
        let md = std::fs::read_to_string(CLI_MD_PATH).expect("docs/CLI.md missing");
        let start = md.find(MD_START).expect("missing cli:auto-start marker") + MD_START.len();
        let end = md.find(MD_END).expect("missing cli:auto-end marker");
        let block = &md[start..end];
        let output = run_codeterm(&["cli-manifest", "--markdown"]);
        assert!(
            output.status.success(),
            "codeterm cli-manifest --markdown failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let markdown =
            String::from_utf8(output.stdout).expect("codeterm cli-manifest --markdown is utf-8");
        let expected = format!("\n{markdown}\n");
        assert_eq!(
            normalize_newlines(block),
            normalize_newlines(&expected),
            "docs/CLI.md auto-block stale — run `codeterm cli-manifest --write`"
        );
    }

    #[test]
    fn chat_ui_command_paths_are_fresh() {
        let committed =
            std::fs::read_to_string(COMMAND_PATHS_PATH).expect("commandPaths.ts missing");
        assert_eq!(
            normalize_newlines(&committed),
            normalize_newlines(&to_command_paths()),
            "commandPaths.ts is stale — run `codeterm cli-manifest --write`"
        );
    }

    const PROMPT_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src-tauri/prompts/codeterm_cli.md"
    );
    const SKILL_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../skills/codeterm/SKILL.md");

    #[test]
    fn shipped_agent_instructions_use_tab_cli_vocabulary() {
        for path in [PROMPT_PATH, SKILL_PATH] {
            let text = std::fs::read_to_string(path).expect("read agent instruction source");
            assert!(
                !text.contains("codeterm pane "),
                "{path} teaches the removed `codeterm pane` namespace"
            );
            assert!(
                !text.contains(" --pane "),
                "{path} teaches the removed `--pane` flag"
            );
        }
    }

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

    // `about` string so docs/CLI.md and chat cards never render a blank line.
    #[test]
    fn every_leaf_command_has_about() {
        fn walk(cmd: &Command, path: &[String], missing: &mut Vec<String>) {
            let mut full = path.to_vec();
            full.push(cmd.get_name().to_string());
            let subs: Vec<&Command> = cmd
                .get_subcommands()
                .filter(|c| c.get_name() != "help" && !c.is_hide_set())
                .collect();
            if subs.is_empty() {
                let about_empty = cmd
                    .get_about()
                    .map(|a| a.to_string().trim().is_empty())
                    .unwrap_or(true);
                if about_empty && !path.is_empty() {
                    missing.push(full.join(" "));
                }
            } else {
                for sub in subs {
                    walk(sub, &full, missing);
                }
            }
        }

        let root = Cli::command();
        let mut missing = Vec::new();
        for sub in root
            .get_subcommands()
            .filter(|c| c.get_name() != "help" && !c.is_hide_set())
        {
            walk(
                sub,
                std::slice::from_ref(&root.get_name().to_string()),
                &mut missing,
            );
        }
        assert!(
            missing.is_empty(),
            "leaf commands missing an `about` description (add a /// doc comment):\n{}",
            missing.join("\n")
        );
    }

    #[test]
    fn manifest_has_core_commands() {
        let m = build();
        let names: Vec<&str> = m.subcommands.iter().map(|c| c.name.as_str()).collect();
        for expected in ["send", "agent", "mem", "tab", "cli-manifest"] {
            assert!(
                names.contains(&expected),
                "missing top-level command: {expected}"
            );
        }
    }
}
