use super::*;
use crate::args::cli::{Cli, Commands};
use clap::Parser;

#[test]
fn blueprint_memory_intents_and_keep_are_explicit() {
    for args in [
        vec!["new", "--like", "watch CI failure"],
        vec!["apply", "--like", "watch CI failure"],
        vec![
            "keep",
            "graph-id",
            "--when",
            "Watch CI failures",
            "--tag",
            "ci",
        ],
    ] {
        assert!(Cli::try_parse_from(["codeterm", "blueprint"].into_iter().chain(args)).is_ok());
    }
    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "apply",
        "local.yaml",
        "--like",
        "CI"
    ])
    .is_err());
    assert!(
        Cli::try_parse_from(["codeterm", "blueprint", "new", "watch", "--like", "CI"]).is_err()
    );
}

#[test]
fn blueprint_keep_when_repeats_one_to_five_times() {
    let parse = |count: usize| {
        let mut args = vec![
            "codeterm".to_owned(),
            "blueprint".into(),
            "keep".into(),
            "graph-id".into(),
            "--tag".into(),
            "ci".into(),
        ];
        for index in 0..count {
            args.push("--when".into());
            args.push(format!("intent {index}"));
        }
        Cli::try_parse_from(args)
    };
    assert!(parse(0).is_err());
    for count in 1..=5 {
        let Commands::Blueprint {
            action: BlueprintAction::Keep { when, .. },
        } = parse(count).unwrap().command
        else {
            panic!("wrong command")
        };
        assert_eq!(when.len(), count);
    }
    assert!(
        parse(6).is_ok(),
        "clap accepts six; the keep request refuses them"
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
fn blueprint_authoring_verbs_and_legacy_flag_refusal() {
    for args in [
        vec![
            "check",
            "github",
            "--set",
            "repo=owner/repo",
            "--set",
            "id=42",
        ],
        vec![
            "apply",
            "watch.yaml",
            "--note",
            "Reports failures",
            "--dry-run",
        ],
        vec!["create", "watch.yaml"],
        vec!["new", "watch"],
        vec!["schema"],
    ] {
        assert!(Cli::try_parse_from(["codeterm", "blueprint"].into_iter().chain(args)).is_ok());
    }
    assert!(
        Cli::try_parse_from(["codeterm", "blueprint", "create", "--probe", "echo ready"]).is_err()
    );
    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "apply",
        "github",
        "--note",
        &"x".repeat(201)
    ])
    .is_err());
    let cli = Cli::try_parse_from(["codeterm", "blueprint", "create", "github"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Blueprint {
            action: BlueprintAction::Apply { .. }
        }
    ));
}

#[test]
fn blueprint_v2_intent_flags_preserve_text_and_bound_limits() {
    let parse = |tail: &[&str]| {
        Cli::try_parse_from(
            ["domios", "blueprint", "new"]
                .into_iter()
                .chain(tail.iter().copied()),
        )
    };
    let Commands::Blueprint {
        action: BlueprintAction::New { intent, limit, .. },
    } = parse(&["--intent", "  opaque\nintent λ  ", "--json"])
        .unwrap()
        .command
    else {
        panic!("wrong command")
    };
    assert_eq!(intent.as_deref(), Some("  opaque\nintent λ  "));
    assert_eq!(limit, 3);
    for limit in ["1", "20"] {
        assert!(parse(&["--intent", "query", "--limit", limit]).is_ok());
    }
    for limit in ["0", "21", "-1", "999999", "x"] {
        assert!(parse(&["--intent", "query", "--limit", limit]).is_err());
    }
    for args in [
        vec!["--intent", ""],
        vec!["--intent", " "],
        vec!["--intent", "query", "watch"],
        vec!["--intent", "query", "--like", "another"],
        vec!["--limit", "1"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
}

#[test]
fn blueprint_v2_call_keep_file_and_id_are_mutually_exclusive() {
    use clap::Parser;
    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "keep",
        "--file",
        "callable.yaml",
        "--when",
        "Increment typed data",
        "--tag",
        "data-transform"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "keep",
        "graph",
        "--file",
        "callable.yaml",
        "--when",
        "Increment typed data",
        "--tag",
        "data-transform"
    ])
    .is_err());
    assert!(Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "keep",
        "--when",
        "Increment typed data",
        "--tag",
        "data-transform"
    ])
    .is_err());
}

#[test]
fn blueprint_keep_tags_are_required_and_vocabulary_is_discoverable() {
    assert!(
        Cli::try_parse_from(["domios", "blueprint", "keep", "graph", "--when", "Watch CI"])
            .is_err()
    );
    assert!(Cli::try_parse_from(["domios", "blueprint", "tags", "--json"]).is_ok());
    let cli = Cli::try_parse_from([
        "domios",
        "blueprint",
        "keep",
        "graph",
        "--when",
        "Watch CI",
        "--tag",
        "ci",
        "--tag",
        "github",
    ])
    .unwrap();
    let Commands::Blueprint {
        action: BlueprintAction::Keep { tags, .. },
    } = cli.command
    else {
        panic!("wrong command")
    };
    assert_eq!(tags, vec!["ci", "github"]);
}
