use crate::Cli;

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
fn plan_task_dispatch_accepts_a_worker_kind_override() {
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
            "--kind",
            kind,
        ])
        .expect("dispatch kind parses");
        let crate::Commands::Plan {
            action:
                crate::PlanAction::Task {
                    action: super::PlanTaskAction::Dispatch { worker_kind, .. },
                },
        } = parsed.command
        else {
            panic!("expected plan task dispatch")
        };
        assert_eq!(worker_kind.as_deref(), Some(kind));
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
