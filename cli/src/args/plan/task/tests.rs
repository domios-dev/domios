use crate::Cli;
use clap::CommandFactory;

#[test]
fn task_update_accepts_repeated_tag_removals_with_additions() {
    use clap::Parser;
    let parsed = Cli::try_parse_from([
        "domios",
        "plan",
        "task",
        "update",
        "task",
        "--tag",
        "work:backend",
        "--untag",
        "build-breaking",
        "--untag",
        "stale",
    ])
    .unwrap();
    let crate::Commands::Plan {
        action:
            crate::PlanAction::Task {
                action:
                    super::PlanTaskAction::Update {
                        tags, remove_tags, ..
                    },
            },
    } = parsed.command
    else {
        panic!("task update expected")
    };
    assert_eq!(tags, ["work:backend"]);
    assert_eq!(remove_tags, ["build-breaking", "stale"]);
}

fn depends_on_help(subcommand: &str) -> String {
    Cli::command()
        .find_subcommand_mut("plan")
        .unwrap()
        .find_subcommand_mut("task")
        .unwrap()
        .find_subcommand_mut(subcommand)
        .unwrap()
        .render_long_help()
        .to_string()
}

#[test]
fn shared_worker_placement_and_extensions_have_explicit_cli_contracts() {
    use clap::Parser;
    for placement in ["shared", "lane"] {
        let parsed = Cli::try_parse_from([
            "domios",
            "plan",
            "task",
            "dispatch",
            "plan",
            "task",
            "--execution-intent",
            "mutating",
            "--placement",
            placement,
        ])
        .unwrap();
        let crate::Commands::Plan {
            action:
                crate::PlanAction::Task {
                    action:
                        super::PlanTaskAction::Dispatch {
                            placement: actual, ..
                        },
                },
        } = parsed.command
        else {
            panic!("dispatch expected")
        };
        assert_eq!(actual.as_deref(), Some(placement));
    }
    assert!(Cli::try_parse_from([
        "domios",
        "plan",
        "task",
        "dispatch",
        "plan",
        "task",
        "--execution-intent",
        "mutating",
        "--placement",
        "shared",
        "--canonical-writer"
    ])
    .is_err());
    assert!(Cli::try_parse_from([
        "domios",
        "plan",
        "task",
        "extend-files",
        "--edit-file",
        "src"
    ])
    .is_ok());
    assert!(Cli::try_parse_from(["domios", "plan", "task", "extend-files"]).is_err());
}

fn dispatch_canonical_writer(extra: &[&str]) -> bool {
    use clap::Parser;
    let mut argv = vec![
        "codeterm",
        "plan",
        "task",
        "dispatch",
        "pl_test",
        "task_test",
        "--execution-intent",
        "mutating",
    ];
    argv.extend_from_slice(extra);
    match Cli::try_parse_from(argv).expect("dispatch parses").command {
        crate::Commands::Plan {
            action:
                crate::PlanAction::Task {
                    action:
                        super::PlanTaskAction::Dispatch {
                            canonical_writer, ..
                        },
                },
        } => canonical_writer,
        _ => panic!("expected plan task dispatch"),
    }
}

#[test]
fn plan_task_dispatch_accepts_description_override() {
    use clap::Parser;
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "dispatch",
        "pl_test",
        "task_test",
        "--execution-intent",
        "mutating",
        "--description",
        "Owner task summary",
    ])
    .expect("dispatch description parses");
    let crate::Commands::Plan {
        action:
            crate::PlanAction::Task {
                action: super::PlanTaskAction::Dispatch { description, .. },
            },
    } = parsed.command
    else {
        panic!("expected plan task dispatch")
    };
    assert_eq!(description, Some(vec!["Owner task summary".into()]));
}

#[test]
fn plan_task_split_parses_children_and_file() {
    use clap::Parser;
    for argv in [
        vec![
            "codeterm", "plan", "task", "split", "pl", "parent", "--child", "alpha", "--child",
            "beta",
        ],
        vec![
            "codeterm",
            "plan",
            "task",
            "split",
            "pl",
            "parent",
            "--from-file",
            "children.json",
        ],
    ] {
        assert!(matches!(
            Cli::try_parse_from(argv)
                .expect("split syntax parses")
                .command,
            crate::Commands::Plan {
                action: crate::PlanAction::Task {
                    action: super::PlanTaskAction::Split { .. }
                }
            }
        ));
    }
}

#[test]
fn canonical_writer_dispatch_flag_is_opt_in_and_off_by_default() {
    assert!(!dispatch_canonical_writer(&[]));
    assert!(dispatch_canonical_writer(&["--canonical-writer"]));
}

#[test]
fn plan_task_dispatch_accepts_a_worker_type() {
    use clap::Parser;
    for kind in ["release_manager", "reviewer"] {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "plan",
            "task",
            "dispatch",
            "plan-id",
            "task-id",
            "--execution-intent",
            "mutating",
            "--canonical-writer",
            "--role",
            kind,
        ])
        .expect("dispatch kind parses");
        let crate::Commands::Plan {
            action:
                crate::PlanAction::Task {
                    action: super::PlanTaskAction::Dispatch { role, .. },
                },
        } = parsed.command
        else {
            panic!("expected plan task dispatch")
        };
        assert_eq!(role.map(|role| role.kind_id).as_deref(), Some(kind));
    }
}

#[test]
fn task_add_help_documents_depends_on_value_types() {
    let help = depends_on_help("add");
    assert!(help.contains("--depends-on"), "{help}");
    assert!(help.contains("full task id"), "{help}");
    assert!(help.contains("handle"), "{help}");
}

#[test]
fn task_update_help_documents_depends_on_value_types() {
    let help = depends_on_help("update");
    assert!(help.contains("--depends-on"), "{help}");
    assert!(help.contains("full task id"), "{help}");
    assert!(help.contains("handle"), "{help}");
}

#[test]
fn task_add_and_update_help_name_model_rationale_storage() {
    for subcommand in ["add", "update"] {
        let help = depends_on_help(subcommand);
        assert!(help.contains("model_rationale"), "{subcommand}: {help}");
    }
}

#[test]
fn task_add_and_update_help_document_external_ref_forms() {
    for subcommand in ["add", "update"] {
        let help = depends_on_help(subcommand);
        assert!(help.contains("--external-ref"), "{subcommand}: {help}");
        assert!(help.contains("full issue URL"), "{subcommand}: {help}");
        assert!(
            help.contains("owner/repository#number"),
            "{subcommand}: {help}"
        );
    }
}

#[test]
fn plan_task_merge_remote_lane_parses_origin_review_attempt() {
    use clap::Parser;
    assert!(matches!(
        Cli::try_parse_from([
            "codeterm",
            "plan",
            "task",
            "merge-remote-lane",
            "plan_1",
            "task_1",
            "attempt_1",
        ])
        .expect("remote lane merge parses")
        .command,
        crate::Commands::Plan {
            action: crate::PlanAction::Task {
                action: super::PlanTaskAction::MergeRemoteLane { .. }
            }
        }
    ));
}

#[test]
fn task_dispatch_and_respawn_accept_launch_profile() {
    use clap::Parser;
    for (verb, extra) in [
        ("dispatch", vec!["--execution-intent", "mutating"]),
        ("respawn", vec!["--attempt", "attempt-source"]),
    ] {
        let mut args = vec![
            "domios",
            "plan",
            "task",
            verb,
            "plan",
            "task",
            "--profile",
            "gemini-cli",
        ];
        args.extend(extra);
        let parsed = Cli::try_parse_from(args).unwrap();
        let crate::Commands::Plan {
            action: crate::PlanAction::Task { action },
        } = parsed.command
        else {
            panic!("expected task command");
        };
        let profile = match action {
            super::PlanTaskAction::Dispatch { launch_profile, .. }
            | super::PlanTaskAction::Respawn { launch_profile, .. } => launch_profile,
            _ => panic!("expected launch command"),
        };
        assert_eq!(profile.as_deref(), Some("gemini-cli"));
    }
}
