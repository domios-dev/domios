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

fn alias_invocations(text: &str) -> Vec<usize> {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        let commands = Cli::command()
            .get_subcommands()
            .map(|command| regex::escape(command.get_name()))
            .collect::<Vec<_>>()
            .join("|");
        regex::Regex::new(&format!(
            r"(?m)(?:^|[^\w/\\.-])(codeterm)(?:\.exe)?[ \t\r\n]+(?:{commands}\b|--[a-z]|<(?:cmd|command)>)"
        ))
        .unwrap()
    });
    pattern
        .captures_iter(text)
        .map(|capture| {
            let start = capture.get(1).unwrap().start();
            text[..start].bytes().filter(|byte| *byte == b'\n').count() + 1
        })
        .collect()
}

fn obsolete_data_paths(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| line.contains("~/.codeterm").then_some(index + 1))
        .collect()
}

fn help_sources(command: &clap::Command, sources: &mut Vec<(String, String)>) {
    let mut rendered = command.clone();
    sources.push((
        format!("help {}", command.get_name()),
        rendered.render_long_help().to_string(),
    ));
    for child in command.get_subcommands() {
        help_sources(child, sources);
    }
}

#[test]
fn shipped_guidance_teaches_the_primary_command() {
    let mut sources = prescription_corpus();
    for directory in ["prod-docs", "plugins", ".agents/skills"] {
        let mut files = Vec::new();
        markdown_files(&repo_path(directory), &mut files);
        sources.extend(files.into_iter().map(|path| {
            let label = corpus_origin(&path, directory);
            (path, label)
        }));
    }
    let mut content: Vec<_> = sources
        .into_iter()
        .filter(|(_, label)| !label.starts_with("src-tauri/prompts/seed/legacy_"))
        .map(|(path, label)| (label, std::fs::read_to_string(path).unwrap()))
        .collect();
    help_sources(&Cli::command(), &mut content);
    let mut failures = Vec::new();
    for (label, text) in content {
        for line in alias_invocations(&text) {
            failures.push(format!("{label}:{line}: teach domios"));
        }
        if !label.starts_with(".agents/skills/") {
            for line in obsolete_data_paths(&text) {
                failures.push(format!("{label}:{line}: use the active Domios data path"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "shipped guidance has obsolete naming:\n{}",
        failures.join("\n")
    );
}
