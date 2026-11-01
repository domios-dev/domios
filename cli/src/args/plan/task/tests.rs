use crate::Cli;
use clap::CommandFactory;

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
