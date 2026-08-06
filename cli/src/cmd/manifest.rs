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
    fn docs_help_teaches_discovery_and_complete_named_read() {
        let help = crate::cmd::docs::render_help().to_lowercase();

        assert!(help.contains("read embedded codeterm documentation"));
        for document in crate::cmd::docs::DOCS {
            assert!(help.contains(&document.name.to_lowercase()));
            assert!(help.contains(&document.purpose.to_lowercase()));
        }
        assert!(!help.contains("--section"));
        assert!(!help.contains("--full"));
        assert!(!help.contains("--head"));
        assert!(!help.contains("preview"));
        assert!(!help.contains("slug"));
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
                "{path} teaches the removed legacy command namespace"
            );
            assert!(
                !text.contains(" --pane "),
                "{path} teaches the removed `--pane` flag"
            );
            assert!(
                text.contains("codeterm tab "),
                "{path} omits the canonical tab namespace"
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

    // A *prescription* is a line a shipped surface offers as a runnable command.
    // Prose that merely names a command mid-sentence is commentary, not a recipe.
    fn prescription<'a>(line: &'a str, head: &str) -> Option<&'a str> {
        let mut s = line.trim().trim_start_matches(['-', '*', '>', ' ']).trim();
        s = s.trim_start_matches('`').trim();
        if let Some(rest) = s.strip_prefix("$ ") {
            s = rest.trim();
        }
        s.strip_prefix(head)?.strip_prefix(' ').map(str::trim_start)
    }

    fn find_sub<'a>(cmd: &'a Command, name: &str) -> Option<&'a Command> {
        cmd.get_subcommands()
            .find(|c| c.get_name() == name || c.get_all_aliases().any(|a| a == name))
    }

    fn is_subcommand_token(tok: &str) -> bool {
        tok.starts_with(|c: char| c.is_ascii_lowercase())
            && tok
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '|')
    }

    // Globals are declared once on the root and inherited, so an option is known
    // if the leaf owns it or any ancestor declares it global.
    fn known_flags(chain: &[&Command]) -> std::collections::HashSet<String> {
        let mut flags: std::collections::HashSet<String> =
            ["--help".to_string(), "--version".to_string()].into();
        let leaf = chain.len() - 1;
        for (depth, cmd) in chain.iter().enumerate() {
            for arg in cmd.get_arguments() {
                if depth != leaf && !arg.is_global_set() {
                    continue;
                }
                if let Some(long) = arg.get_long() {
                    flags.insert(format!("--{long}"));
                }
                for alias in arg.get_all_aliases().unwrap_or_default() {
                    flags.insert(format!("--{alias}"));
                }
            }
        }
        flags
    }

    fn check_prescription(
        root: &Command,
        base: &[String],
        rest: &str,
        origin: &str,
        bad: &mut Vec<String>,
    ) {
        let mut chain: Vec<&Command> = vec![root];
        for seg in base {
            let Some(next) = find_sub(chain[chain.len() - 1], seg) else {
                return;
            };
            chain.push(next);
        }
        let mut walked: Vec<String> = base.to_vec();
        let render = |walked: &[String]| {
            if walked.is_empty() {
                "codeterm".to_string()
            } else {
                format!("codeterm {}", walked.join(" "))
            }
        };

        let mut toks = rest.split_whitespace().peekable();
        while let Some(tok) = toks.peek().copied() {
            let cur = chain[chain.len() - 1];
            if cur.get_subcommands().next().is_none() || !is_subcommand_token(tok) {
                break;
            }
            let mut next = None;
            for alt in tok.split('|').filter(|a| !a.is_empty()) {
                match find_sub(cur, alt) {
                    Some(c) => next = Some(c),
                    None => {
                        bad.push(format!(
                            "{origin}: `{alt}` is not a subcommand of `{}`",
                            render(&walked)
                        ));
                        return;
                    }
                }
            }
            let Some(next) = next else { break };
            walked.push(tok.to_string());
            chain.push(next);
            toks.next();
        }

        let flags = known_flags(&chain);
        for tok in toks {
            if tok.starts_with('#') {
                break;
            }
            let Some(flag) = tok.strip_prefix("--") else {
                continue;
            };
            let flag = flag
                .split('=')
                .next()
                .unwrap_or_default()
                .trim_end_matches([',', '.', ';', ':', ')', ']', '`', '"', '\'']);
            if flag.is_empty()
                || !flag
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                continue;
            }
            if !flags.contains(&format!("--{flag}")) {
                bad.push(format!(
                    "{origin}: `--{flag}` is not an option of `{}`",
                    render(&walked)
                ));
            }
        }
    }

    fn check_help_text(root: &Command, cmd: &Command, path: &[String], bad: &mut Vec<String>) {
        let mut texts: Vec<String> = Vec::new();
        for text in [cmd.get_about(), cmd.get_long_about()]
            .into_iter()
            .flatten()
        {
            texts.push(text.to_string());
        }
        for arg in cmd.get_arguments() {
            for text in [arg.get_help(), arg.get_long_help()].into_iter().flatten() {
                texts.push(text.to_string());
            }
        }
        let origin = format!("help `{}`", {
            let mut p = vec!["codeterm".to_string()];
            p.extend(path.iter().cloned());
            p.join(" ")
        });
        for text in &texts {
            for line in text.lines() {
                if let Some(rest) = prescription(line, "codeterm") {
                    check_prescription(root, &[], rest, &origin, bad);
                }
                // Help for `agent spawn` writes its examples as `spawn ...`.
                if let Some(own) = path.last() {
                    if let Some(rest) = prescription(line, own) {
                        check_prescription(root, path, rest, &origin, bad);
                    }
                }
            }
        }
        for sub in cmd
            .get_subcommands()
            .filter(|c| c.get_name() != "help" && !c.is_hide_set())
        {
            let mut child = path.to_vec();
            child.push(sub.get_name().to_string());
            check_help_text(root, sub, &child, bad);
        }
    }

    // The consistency oracle for everything the CLI ships as instruction: an
    // agent follows a prescribed command literally, so one that cannot run is a
    // failed tool call, not a typo.
    #[test]
    fn shipped_prescriptions_are_runnable() {
        let root = Cli::command();
        let mut bad: Vec<String> = Vec::new();

        for document in crate::cmd::docs::DOCS {
            for (idx, line) in document.body.lines().enumerate() {
                let Some(rest) = prescription(line, "codeterm") else {
                    continue;
                };
                let origin = format!("docs {}:{}", document.name, idx + 1);
                check_prescription(&root, &[], rest, &origin, &mut bad);
            }
        }

        check_help_text(&root, &root, &[], &mut bad);

        assert!(
            bad.is_empty(),
            "shipped text prescribes actions the CLI cannot perform:\n{}",
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

    // Contract for the flags the CLI-guidance prompt documents: introspect the
    // live clap tree so a renamed/removed flag fails here, not in an agent's hands.
    fn subcommand<'a>(root: &'a Command, path: &[&str]) -> Option<&'a Command> {
        let mut cur = root;
        for seg in path {
            cur = find_sub(cur, seg)?;
        }
        Some(cur)
    }

    fn has_long(cmd: &Command, long: &str) -> bool {
        cmd.get_arguments().any(|a| {
            a.get_long() == Some(long) || a.get_all_aliases().unwrap_or_default().contains(&long)
        })
    }

    #[test]
    fn documented_plan_task_flags_exist_on_their_subcommands() {
        let root = Cli::command();

        let add = subcommand(&root, &["plan", "task", "add"]).expect("plan task add exists");
        for flag in [
            "name",
            "sections",
            "criterion",
            "body",
            "body-file",
            "test-plan",
            "test-plan-file",
            "correctness",
            "correctness-file",
            "executor-role",
            "personality",
            "role-guidance",
            "execution-intent",
        ] {
            assert!(has_long(add, flag), "plan task add lost --{flag}");
        }
        assert!(!has_long(add, "acceptance"), "plan task add must not grow --acceptance");
        assert!(!has_long(add, "title"), "plan task add uses --name, not --title");

        let dispatch =
            subcommand(&root, &["plan", "task", "dispatch"]).expect("plan task dispatch exists");
        assert!(has_long(dispatch, "execution-intent"));
        assert!(!has_long(dispatch, "brief"), "dispatch has no --brief; the brief is the task body");

        let update =
            subcommand(&root, &["plan", "task", "update"]).expect("plan task update exists");
        assert!(has_long(update, "model"));

        assert!(subcommand(&root, &["plan", "task", "show"]).is_some());
    }

    #[test]
    fn documented_plan_flags_exist_on_their_subcommands() {
        let root = Cli::command();

        let create = subcommand(&root, &["plan", "create"]).expect("plan create exists");
        assert!(has_long(create, "title"));
        assert!(has_long(create, "from-spec"));
        assert!(!has_long(create, "goal"), "plan create has no --goal; goal lives in `plan goal set`");

        let goal_set =
            subcommand(&root, &["plan", "goal", "set"]).expect("plan goal set exists");
        assert!(has_long(goal_set, "vector"), "plan goal set requires --vector");

        let accept =
            subcommand(&root, &["plan", "review", "accept"]).expect("plan review accept exists");
        assert!(has_long(accept, "note"), "review accept uses --note");
        assert!(!has_long(accept, "reason"), "review accept uses --note, not --reason");

        let mode = subcommand(&root, &["plan", "mode"]).expect("plan mode exists");
        assert!(has_long(mode, "set"), "plan mode keeps --set alongside the positional");

        // Teams are minted with the plan or via `team pair`; there is no `team create`.
        assert!(subcommand(&root, &["team", "pair"]).is_some());
        assert!(subcommand(&root, &["team", "create"]).is_none());
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
