use super::*;
use crate::args::cli::{Cli, Commands};
use clap::Parser;

#[test]
fn blueprint_rejects_removed_command_and_route() {
    assert!(Cli::try_parse_from(["codeterm", "sentinel", "list"]).is_err());
    assert!(parse_route("done=sentinel:exact-id").is_err());
    assert_eq!(
        parse_route("done=blueprint:exact-id").unwrap(),
        "done=blueprint:exact-id"
    );
}

#[test]
fn sentinel_create_parse_and_json() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "create",
        "--name",
        "ci-result-watch",
        "--probe",
        "check-ci",
        "--question",
        "CI state?",
        "--buckets",
        "pending,passed,failed",
        "--every",
        "20s",
        "--escalation-timeout-ms",
        "5000",
        "--route",
        "passed=send:Review",
        "--route",
        "failed=stop",
        "--route",
        "pending=none",
        "--rearm-on-change",
        "pending",
        "--rearm-on-change",
        "failed",
        "--match",
        "passed=^success$",
        "--until",
        "passed",
        "--timeout",
        "1h",
        "--json",
    ])
    .unwrap();
    assert!(cli.json);
    let Commands::Blueprint {
        action:
            BlueprintAction::Create {
                buckets,
                routes,
                rearm_on_change,
                matches,
                escalation_timeout_ms,
                ..
            },
    } = cli.command
    else {
        panic!("wrong command")
    };
    assert_eq!(buckets.unwrap(), ["pending", "passed", "failed"]);
    assert_eq!(routes.len(), 3);
    assert_eq!(rearm_on_change, ["pending", "failed"]);
    assert_eq!(matches, ["passed=^success$"]);
    assert_eq!(escalation_timeout_ms, Some(5000));
    for args in [
        vec!["codeterm", "blueprint", "list", "--json"],
        vec!["codeterm", "blueprint", "show", "id", "--json"],
        vec!["codeterm", "blueprint", "stop", "id", "--json"],
        vec!["codeterm", "blueprint", "history", "id", "--json"],
    ] {
        assert!(Cli::try_parse_from(args).unwrap().json);
    }
}

#[test]
fn sentinel_observe_parse_and_probe_conflict() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "create",
        "--name",
        "agent-state-watch",
        "--observe",
        "tab:bound-agent",
        "--question",
        "tab state?",
        "--buckets",
        "waiting",
        "--match",
        "waiting=.*",
    ])
    .unwrap();
    let Commands::Blueprint {
        action: BlueprintAction::Create { probe, observe, .. },
    } = cli.command
    else {
        panic!("wrong command")
    };
    assert_eq!(probe, None);
    assert_eq!(observe.as_deref(), Some("tab:bound-agent"));
    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "create",
        "--name",
        "invalid-conflict-watch",
        "--probe",
        "check",
        "--observe",
        "tab:bound-agent",
        "--question",
        "state?",
        "--buckets",
        "waiting",
    ])
    .is_err());
    for invalid in ["cron:check", "cron-run:", "tab:", "tab:bad\nname"] {
        assert!(parse_observe(invalid).is_err());
    }
    assert_eq!(
        parse_observe("cron-run:nightly"),
        Ok("cron-run:nightly".into())
    );
}

#[test]
fn sentinel_update_parse_requires_expect_version_and_file() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "update",
        "sentinel_a",
        "--expect-version",
        "4",
        "--from-file",
        "definition.json",
        "--cause",
        "transition_9",
        "--json",
    ])
    .unwrap();
    let Commands::Blueprint {
        action:
            BlueprintAction::Update {
                id,
                expect_version,
                from_file,
                cause,
                ..
            },
    } = cli.command
    else {
        panic!("wrong command")
    };
    assert_eq!(id, "sentinel_a");
    assert_eq!(expect_version, 4);
    assert_eq!(from_file, std::path::PathBuf::from("definition.json"));
    assert_eq!(cause.as_deref(), Some("transition_9"));
    assert!(Cli::try_parse_from(["codeterm", "blueprint", "update", "id"]).is_err());
}

#[test]
fn sentinel_escalate_resolve_parse_requires_stage_and_transition_ids() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "escalation",
        "resolve",
        "sentinel_a",
        "transition_9",
        "--json",
    ])
    .unwrap();
    let Commands::Blueprint {
        action:
            BlueprintAction::Escalation {
                action: BlueprintEscalationAction::Resolve { id, transition_id },
            },
    } = cli.command
    else {
        panic!("wrong command")
    };
    assert_eq!(id, "sentinel_a");
    assert_eq!(transition_id, "transition_9");
    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "escalation",
        "resolve",
        "sentinel_a",
    ])
    .is_err());
}

#[test]
fn sentinel_routes_closed_set() {
    for action in [
        "notify",
        "blueprint:sentinel_child",
        "send:tab",
        "cron:check",
        "stop",
    ] {
        assert!(parse_route(&format!("passed={action}")).is_ok());
    }
    for action in ["sh:evil", "send:", "cron:", "notify;evil", "exec", ""] {
        assert!(parse_route(&format!("passed={action}")).is_err());
    }
}

#[test]
fn sentinel_create_requires_a_short_non_raw_name() {
    let base = [
        "codeterm",
        "blueprint",
        "create",
        "--probe",
        "status",
        "--question",
        "state?",
        "--buckets",
        "done",
    ];
    assert!(Cli::try_parse_from(base).is_err());
    for name in ["ab", "sentinel_5d22889a8898"] {
        assert!(Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "create",
            "--name",
            name,
            "--probe",
            "status",
            "--question",
            "state?",
            "--buckets",
            "done",
        ])
        .is_err());
    }
    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "create",
        "--name",
        "ci-green-watch",
        "--probe",
        "status",
        "--question",
        "state?",
        "--buckets",
        "done",
    ])
    .is_ok());
}

#[test]
fn sentinel_composition_flags_refuse_mixed_input_and_out_of_range_budgets() {
    let prefix = [
        "codeterm",
        "blueprint",
        "create",
        "--name",
        "budget-validation-watch",
        "--probe",
        "status",
        "--question",
        "state?",
        "--buckets",
        "done",
    ];
    for suffix in [
        vec!["--route", "done=notify", "--handlers-file", "handlers.json"],
        vec!["--max-transitions", "0"],
        vec!["--max-transitions", "257"],
        vec!["--max-actions", "0"],
        vec!["--max-actions", "1025"],
    ] {
        assert!(Cli::try_parse_from(prefix.into_iter().chain(suffix)).is_err());
    }
}

fn input_prefix() -> Vec<&'static str> {
    vec![
        "codeterm",
        "blueprint",
        "create",
        "--name",
        "voice-note-watch",
        "--input",
        "audio",
        "--question",
        "transcribed?",
        "--buckets",
        "done",
    ]
}

#[test]
fn sentinel_input_flags_parse_and_conflict() {
    let cli = Cli::try_parse_from(
        input_prefix()
            .into_iter()
            .chain([
                "--input-file",
                "clips/voice.wav",
                "--convert-to",
                "text",
                "--convert-timeout",
                "30s",
            ])
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let Commands::Blueprint {
        action:
            BlueprintAction::Create {
                input,
                input_file,
                input_url,
                input_text,
                convert_to,
                convert_timeout,
                ..
            },
    } = cli.command
    else {
        panic!("wrong command")
    };
    assert_eq!(input.as_deref(), Some("audio"));
    assert_eq!(
        input_file.as_deref(),
        Some(std::path::Path::new("clips/voice.wav"))
    );
    assert_eq!(input_url, None);
    assert_eq!(input_text, None);
    assert_eq!(convert_to.as_deref(), Some("text"));
    assert_eq!(convert_timeout, Some(30_000));

    for invalid in [
        vec!["--input", "audio", "--probe", "status"],
        vec!["--input", "audio", "--observe", "tab:bound"],
        vec![
            "--input-file",
            "clips/voice.wav",
            "--input-url",
            "https://e.com/a",
        ],
        vec!["--input-file", "clips/voice.wav", "--input-text", "hello"],
        vec!["--input", "audio"],
        vec!["--input", "midi", "--input-file", "clips/voice.wav"],
        vec!["--input", "audio", "--convert-to", "video"],
        vec!["--input", "audio", "--input-url", "ftp://example.com/a"],
        vec!["--input", "audio", "--convert-timeout", "forever"],
        vec![
            "--input",
            "audio",
            "--input-file",
            "clips/voice.wav",
            "--rearm-on-change",
            "done",
        ],
    ] {
        assert!(
            Cli::try_parse_from(input_prefix().into_iter().chain(invalid.iter().copied()))
                .is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn sentinel_input_every_conflicts_only_when_explicit() {
    assert!(Cli::try_parse_from(input_prefix().into_iter().chain([
        "--input-file",
        "clips/voice.wav",
        "--every",
        "20s"
    ]))
    .is_err());
    assert!(Cli::try_parse_from(
        input_prefix()
            .into_iter()
            .chain(["--input-file", "clips/voice.wav"])
    )
    .is_ok());
}

#[test]
fn blueprint_pipeline_create_nodes_and_log_parse() {
    let spec = Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "create",
        "--spec",
        "pipeline.yaml",
        "--owner",
        "team",
        "--dry-run",
    ])
    .unwrap();
    let Commands::Blueprint {
        action:
            BlueprintAction::Create {
                spec,
                owner,
                dry_run,
                ..
            },
    } = spec.command
    else {
        panic!("wrong command")
    };
    assert_eq!(spec, Some(std::path::PathBuf::from("pipeline.yaml")));
    assert_eq!(owner.as_deref(), Some("team"));
    assert!(dry_run);

    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "create",
        "--recipe",
        "echo",
        "--set",
        "message=ready"
    ])
    .is_ok());
    assert!(Cli::try_parse_from(["codeterm", "blueprint", "nodes", "--json"]).is_ok());
    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "log",
        "blueprint-id",
        "--limit",
        "200",
        "--json"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "log",
        "blueprint-id",
        "--limit",
        "201"
    ])
    .is_err());
}
#[test]
fn blueprint_model_classification_is_explicit_and_conflicts_with_patterns() {
    let base = [
        "codeterm",
        "blueprint",
        "create",
        "--name",
        "model-watch",
        "--probe",
        "status",
        "--question",
        "state?",
        "--buckets",
        "ready",
    ];
    let parsed =
        Cli::try_parse_from(base.into_iter().chain(["--model-classification"])).unwrap();
    assert!(matches!(
        parsed.command,
        Commands::Blueprint {
            action: BlueprintAction::Create {
                model_classification: true,
                ..
            }
        }
    ));
    let parsed = Cli::try_parse_from(base).unwrap();
    assert!(matches!(
        parsed.command,
        Commands::Blueprint {
            action: BlueprintAction::Create {
                model_classification: false,
                ..
            }
        }
    ));
    assert!(Cli::try_parse_from(base.into_iter().chain([
        "--model-classification",
        "--match",
        "ready=^ready$"
    ]))
    .is_err());
}

#[test]
fn blueprint_notes_parse_on_create_and_update_and_reject_unbounded_text() {
    let cli = Cli::try_parse_from([
        "codeterm", "blueprint", "create", "--recipe", "echo", "--note", "Reports failed jobs",
    ]).unwrap();
    assert!(matches!(cli.command, Commands::Blueprint { action: BlueprintAction::Create { note: Some(note), .. } } if note == "Reports failed jobs"));
    let cli = Cli::try_parse_from([
        "codeterm", "blueprint", "update", "blueprint-a", "--expect-version", "2", "--from-file", "definition.json", "--note", "Next release watcher",
    ]).unwrap();
    assert!(matches!(cli.command, Commands::Blueprint { action: BlueprintAction::Update { note: Some(note), .. } } if note == "Next release watcher"));
    assert!(parse_blueprint_note(&"a".repeat(200)).is_ok());
    assert!(parse_blueprint_note(&"a".repeat(201)).is_err());
    assert!(parse_blueprint_note("line one\nline two").is_err());
}
