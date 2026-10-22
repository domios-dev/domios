use super::*;

#[test]
fn canonical_program_name_is_independent_of_argv0() {
    assert_eq!(build().name, "domios");
    for args in [
        vec!["--help"],
        vec!["--version"],
        vec!["agent", "report", "--help"],
        vec!["unknown-command"],
        vec!["tab", "unknown-command"],
    ] {
        let render = |program: &str| {
            Cli::command()
                .try_get_matches_from(std::iter::once(program).chain(args.iter().copied()))
                .unwrap_err()
                .to_string()
        };
        let expected = render("domios");
        assert!(expected.contains("domios"), "{args:?}: {expected}");
        for alias in ["codeterm", "codeterm.exe", "/tmp/custom-cli"] {
            assert_eq!(render(alias), expected, "{alias}: {args:?}");
        }
    }
}

#[test]
fn both_program_names_dispatch_to_identical_commands() {
    for args in [
        vec!["docs", "API-CLI"],
        vec!["cli-manifest", "--json"],
        vec!["tab", "list"],
        vec!["--json", "agent", "report", "--status", "in_review"],
    ] {
        let parse = |program: &str| {
            Cli::command()
                .try_get_matches_from(std::iter::once(program).chain(args.iter().copied()))
                .unwrap()
        };
        assert_eq!(parse("domios"), parse("codeterm"), "{args:?}");
    }
}

fn alias_invocations(text: &str) -> Vec<usize> {
    let commands = Cli::command()
        .get_subcommands()
        .map(|command| regex::escape(command.get_name()))
        .collect::<Vec<_>>()
        .join("|");
    let pattern = regex::Regex::new(&format!(
        r"(?m)(?:^|[^\w/\\.-])(codeterm)(?:\.exe)?[ \t\r\n]+(?:{commands}\b|--[a-z]|<(?:cmd|command)>)"
    ))
    .unwrap();
    pattern
        .captures_iter(text)
        .map(|capture| {
            let start = capture.get(1).unwrap().start();
            text[..start].bytes().filter(|byte| *byte == b'\n').count() + 1
        })
        .collect()
}

#[test]
fn shipped_guidance_teaches_the_primary_command() {
    let mut sources = prescription_corpus();
    for directory in ["prod-docs", ".agents/skills"] {
        let mut files = Vec::new();
        markdown_files(&repo_path(directory), &mut files);
        sources.extend(files.into_iter().map(|path| {
            let label = corpus_origin(&path, directory);
            (path, label)
        }));
    }
    let mut failures = Vec::new();
    for (path, label) in sources {
        if label.starts_with("src-tauri/prompts/seed/legacy_") {
            continue;
        }
        let text = std::fs::read_to_string(path).unwrap();
        for line in alias_invocations(&text) {
            failures.push(format!("{label}:{line}"));
        }
    }
    assert!(
        failures.is_empty(),
        "teach domios command invocations:\n{}",
        failures.join("\n")
    );
}

#[test]
fn guidance_guard_finds_inline_and_fenced_commands_without_renaming_identifiers() {
    assert_eq!(alias_invocations("Run `codeterm docs API-CLI`."), [1]);
    assert_eq!(alias_invocations("```bash\ncodeterm tab list\n```"), [2]);
    assert_eq!(alias_invocations("codeterm\nconfig get"), [1]);
    assert!(alias_invocations(
        "Use `domios docs API-CLI`; `codeterm` remains an alias. skills/codeterm docs, codeterm-plugins, CODETERM_TAB_ID, codeterm_cli.md"
    ).is_empty());
}
