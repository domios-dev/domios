use super::*;

#[test]
fn explicit_empty_task_option_suppresses_binding_hydration() {
    let separated = vec!["codeterm".into(), "--task".into(), "".into()];
    let joined = vec!["codeterm".into(), "--task=".into()];
    assert!(option_is_present(&separated, "--task"));
    assert!(option_is_present(&joined, "--task"));
    assert!(!option_is_present(&separated, "--plan"));
}

#[test]
fn plan_task_body_preserves_dash_leading_text() {
    let args = vec![
        "codeterm".to_string(),
        "plan".to_string(),
        "task".to_string(),
        "add".to_string(),
        "plan_1".to_string(),
        "--body".to_string(),
        "- starts with a dash".to_string(),
    ];
    let cli = Cli::try_parse_from(&args).expect("dash-leading text must parse");
    let Commands::Plan {
        action:
            PlanAction::Task {
                action: args::plan::PlanTaskAction::Add { body, .. },
            },
    } = cli.command
    else {
        panic!("expected plan task add");
    };
    assert_eq!(body.as_deref(), Some("- starts with a dash"));
}

#[test]
fn normalizes_agent_spawn_unquoted_label_words() {
    let args = normalize_agent_spawn_label_args(
        [
            "codeterm",
            "agent",
            "spawn",
            "claude",
            "--title",
            "perf",
            "brainstorm",
            "opus",
            "--task",
            "review",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    assert!(args.contains(&"--title=perf brainstorm opus".to_string()));
    let cli = Cli::try_parse_from(args).expect("normalized spawn parses");
    match cli.command {
        Commands::Agent {
            action: AgentAction::Spawn {
                title, task, agent, ..
            },
        } => {
            assert_eq!(agent.as_deref(), Some("claude"));
            assert_eq!(
                title.map(|parts| parts.join(" ")).as_deref(),
                Some("perf brainstorm opus")
            );
            assert_eq!(task.as_deref(), Some("review"));
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn normalizes_agent_spawn_title_before_global_json_flag() {
    let args = normalize_agent_spawn_label_args(
        [
            "codeterm", "agent", "spawn", "claude", "--title", "Scout", "--json",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    let cli = Cli::try_parse_from(args).expect("global json flag after title parses");
    assert!(cli.json);
    match cli.command {
        Commands::Agent {
            action: AgentAction::Spawn { title, .. },
        } => assert_eq!(title.map(|parts| parts.join(" ")).as_deref(), Some("Scout")),
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn normalizes_quoted_multiword_agent_spawn_title() {
    let args = normalize_agent_spawn_label_args(
        [
            "codeterm",
            "agent",
            "spawn",
            "claude",
            "--title",
            "Two Words",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    let cli = Cli::try_parse_from(args).expect("quoted multiword title parses");
    match cli.command {
        Commands::Agent {
            action: AgentAction::Spawn { title, .. },
        } => assert_eq!(
            title.map(|parts| parts.join(" ")).as_deref(),
            Some("Two Words")
        ),
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn normalizes_agent_spawn_dash_leading_label_without_eating_known_flags() {
    let args = normalize_agent_spawn_label_args(
        [
            "codeterm",
            "agent",
            "spawn",
            "claude",
            "--title",
            "--literal",
            "flag",
            "--task",
            "echo",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    assert!(args.contains(&"--title=--literal flag".to_string()));
    let cli = Cli::try_parse_from(args).expect("normalized dash-leading title parses");
    match cli.command {
        Commands::Agent {
            action: AgentAction::Spawn { title, task, .. },
        } => {
            assert_eq!(
                title.map(|parts| parts.join(" ")).as_deref(),
                Some("--literal flag")
            );
            assert_eq!(task.as_deref(), Some("echo"));
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn normalizes_agent_spawn_label_without_eating_capability_flags() {
    let args = normalize_agent_spawn_label_args(
        [
            "codeterm",
            "agent",
            "spawn",
            "codex",
            "--title",
            "Team routing smoke",
            "--reasoning-effort",
            "low",
            "--non-user-facing",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    let cli = Cli::try_parse_from(args).expect("normalized spawn parses");
    match cli.command {
        Commands::Agent {
            action:
                AgentAction::Spawn {
                    title,
                    reasoning_effort,
                    non_user_facing,
                    ..
                },
        } => {
            assert_eq!(
                title.map(|parts| parts.join(" ")).as_deref(),
                Some("Team routing smoke")
            );
            assert_eq!(reasoning_effort.as_deref(), Some("low"));
            assert!(non_user_facing);
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn agent_spawn_provider_args_bypass_label_normalization() {
    let original: Vec<String> = [
        "codeterm",
        "agent",
        "spawn",
        "claude",
        "--role",
        "Worker",
        "--args",
        "--title",
        "provider",
        "session",
        "--description",
        "verbatim",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let args = normalize_agent_spawn_label_args(original.clone());

    assert_eq!(args, original);
    let cli = Cli::try_parse_from(args).expect("provider args should parse");
    match cli.command {
        Commands::Agent {
            action: AgentAction::Spawn { role, args, .. },
        } => {
            assert!(matches!(role, Some(crate::args::agent::SpawnRole::Worker)));
            assert_eq!(
                args,
                Some(vec![
                    "--title".into(),
                    "provider".into(),
                    "session".into(),
                    "--description".into(),
                    "verbatim".into(),
                ])
            );
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn parses_domios_route_text_as_an_agent_spawn_title() {
    let args = normalize_agent_spawn_label_args(
        [
            "codeterm",
            "agent",
            "spawn",
            "claude",
            "--title",
            "<domios",
            "from=tab>",
            "--task",
            "echo",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    assert!(args.contains(&"--title=<domios from=tab>".to_string()));
    let cli = Cli::try_parse_from(args).expect("normalized route title parses");
    match cli.command {
        Commands::Agent {
            action: AgentAction::Spawn { title, task, .. },
        } => {
            assert_eq!(
                title.map(|parts| parts.join(" ")).as_deref(),
                Some("<domios from=tab>")
            );
            assert_eq!(task.as_deref(), Some("echo"));
        }
        _ => panic!("expected agent spawn"),
    }
}
