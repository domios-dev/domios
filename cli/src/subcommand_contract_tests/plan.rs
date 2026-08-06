use super::*;
use crate::args::plan::{PlanAction, PlanHandoffAction};

#[derive(clap::Parser)]
struct PlanOnly {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    action: PlanAction,
}

struct ParsedPlan {
    json: bool,
    command: Commands,
}

fn parse_plan_cli_result<const N: usize>(args: [&str; N]) -> Result<ParsedPlan, clap::Error> {
    let mut stripped = Vec::with_capacity(N);
    stripped.push("plan");
    for arg in args.into_iter().skip(1) {
        if arg != "plan" {
            stripped.push(arg);
        }
    }
    PlanOnly::try_parse_from(stripped).map(|parsed| ParsedPlan {
        json: parsed.json,
        command: Commands::Plan {
            action: parsed.action,
        },
    })
}

#[test]
fn qa_next_claim_requires_note_and_teaches_only_positional_plan_id() {
    let error = PlanOnly::try_parse_from(["plan", "qa", "next", "pl_1", "--claim"])
        .err()
        .expect("claim without note must fail");
    let rendered = error.to_string();
    assert!(rendered.contains("--note"), "{rendered}");
    assert!(!rendered.contains("--plan"), "{rendered}");
    assert_eq!(rendered.matches("<PLAN_ID>").count(), 1, "{rendered}");
    assert!(PlanOnly::try_parse_from([
        "plan",
        "qa",
        "next",
        "pl_1",
        "--claim",
        "--note",
        "canonical gate"
    ])
    .is_ok());
}

#[test]
fn plan_search_kind_arguments() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "search",
        "delivery ledger evidence",
        "--kind",
        "task",
        "--scope",
        "workspace:ws_1",
    ])
    .expect("plan search filters parse");
    match parsed.command {
        Commands::Plan {
            action: PlanAction::Search {
                kind, scope, limit, ..
            },
        } => {
            assert_eq!(kind.as_deref(), Some("task"));
            assert_eq!(scope.as_deref(), Some("workspace:ws_1"));
            assert_eq!(limit, 10);
        }
        _ => panic!("expected plan search"),
    }

    let error = match parse_plan_cli_result([
        "codeterm",
        "plan",
        "search",
        "delivery ledger evidence",
        "--kind",
        "nonsense",
    ]) {
        Ok(_) => panic!("unknown plan search kind must fail"),
        Err(error) => error,
    };
    let message = error.to_string();
    for kind in [
        "plan",
        "plan_section",
        "spec",
        "spec_section",
        "task",
        "stride",
    ] {
        assert!(message.contains(kind), "missing {kind} in: {message}");
    }
}

#[test]
fn plan_record_parses_batch_sugar() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "record",
        "pl_1",
        "--upsert-task",
        "handoff-cli",
        "--name",
        "Handoff CLI",
        "--sections",
        "cli,backend",
        "--status",
        "in_progress",
        "--body-file",
        "body.md",
        "--step",
        "decision:ship batch first",
        "--link",
        "docs/spec.md",
        "--key",
        "stable-key",
    ])
    .expect("plan record parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Record {
                    plan_id,
                    upsert_task,
                    name,
                    sections,
                    status,
                    body_file,
                    steps,
                    links,
                    key,
                    ..
                },
        } => {
            assert_eq!(plan_id.as_deref(), Some("pl_1"));
            assert_eq!(upsert_task.as_deref(), Some("handoff-cli"));
            assert_eq!(name.as_deref(), Some("Handoff CLI"));
            assert_eq!(sections.as_deref(), Some("cli,backend"));
            assert_eq!(status.as_deref(), Some("in_progress"));
            assert_eq!(body_file.as_deref(), Some("body.md"));
            assert_eq!(steps, vec!["decision:ship batch first"]);
            assert_eq!(links, vec!["docs/spec.md"]);
            assert_eq!(key.as_deref(), Some("stable-key"));
        }
        _ => panic!("expected plan record"),
    }
}

#[test]
fn plan_delete_confirmation_flags_parse() {
    #[derive(clap::Parser)]
    struct PlanOnly {
        #[command(subcommand)]
        action: PlanAction,
    }

    let parsed = PlanOnly::try_parse_from(["plan", "delete", "pl_1", "--yes"])
        .expect("plan delete --yes parses");
    match parsed.action {
        PlanAction::Delete { plan_id, yes } => {
            assert_eq!(plan_id, "pl_1");
            assert!(yes);
        }
        _ => panic!("expected plan delete"),
    }

    let parsed = PlanOnly::try_parse_from(["plan", "delete", "pl_1", "--confirm"])
        .expect("plan delete --confirm parses");
    match parsed.action {
        PlanAction::Delete { plan_id, yes } => {
            assert_eq!(plan_id, "pl_1");
            assert!(yes);
        }
        _ => panic!("expected plan delete"),
    }
}

#[test]
fn plan_pause_parses_without_park_alias() {
    #[derive(clap::Parser)]
    struct PlanOnly {
        #[command(subcommand)]
        action: PlanAction,
    }

    let parsed =
        PlanOnly::try_parse_from(["plan", "pause", "pl_1", "--reason", "waiting for quota"])
            .expect("plan pause parses");
    match parsed.action {
        PlanAction::Pause { plan_id, reason } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(reason, "waiting for quota");
        }
        _ => panic!("expected plan pause"),
    }
}

#[test]
fn plan_task_update_parses_sections() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "task",
        "update",
        "pl_1",
        "task_1",
        "--sections",
        "sec_a,sec_b",
    ])
    .expect("plan task update parses sections");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Update { sections, .. },
                },
        } => assert_eq!(sections.as_deref(), Some("sec_a,sec_b")),
        _ => panic!("expected plan task update"),
    }
}

#[test]
fn plan_task_close_help_describes_the_orchestrator_override() {
    let command = PlanOnly::command();
    let close = command
        .find_subcommand("task")
        .and_then(|task| task.find_subcommand("close"))
        .expect("plan task close must exist");
    let about = close
        .get_about()
        .expect("plan task close must explain its authority");
    let about = about.to_string();

    assert!(about.contains("any lifecycle state"), "{about}");
    assert!(about.contains("Plan Orchestrator"), "{about}");
    assert!(about.contains("owner-epoch"), "{about}");
    assert!(about.contains("open subtasks"), "{about}");
}

#[test]
fn plan_task_update_parses_quality_fields() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "task",
        "update",
        "pl_1",
        "task_1",
        "--criterion",
        "no separate task button",
        "--test-plan",
        "focused CLI update smoke",
        "--qa-plan-file",
        "qa-plan.md",
        "--correctness",
        "one Plan action focuses optional task target",
    ])
    .expect("plan task update parses quality fields");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        crate::args::plan::PlanTaskAction::Update {
                            criterion,
                            test_plan,
                            qa_plan_file,
                            correctness,
                            ..
                        },
                },
        } => {
            assert_eq!(criterion, vec!["no separate task button"]);
            assert_eq!(test_plan.as_deref(), Some("focused CLI update smoke"));
            assert_eq!(qa_plan_file.as_deref(), Some("qa-plan.md"));
            assert_eq!(
                correctness.as_deref(),
                Some("one Plan action focuses optional task target")
            );
        }
        _ => panic!("expected plan task update"),
    }
}

#[test]
fn plan_task_update_parses_dependency_flags() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "task",
        "update",
        "pl_1",
        "task_1",
        "--depends-on",
        "task_a",
        "--depends-on",
        "task_b",
    ])
    .expect("plan task update parses dependencies");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        crate::args::plan::PlanTaskAction::Update {
                            depends_on,
                            clear_dependencies,
                            ..
                        },
                },
        } => {
            assert_eq!(depends_on, vec!["task_a", "task_b"]);
            assert!(!clear_dependencies);
        }
        _ => panic!("expected plan task update"),
    }
}

#[test]
fn plan_task_update_parses_clear_dependencies_flag() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "task",
        "update",
        "pl_1",
        "task_1",
        "--clear-dependencies",
    ])
    .expect("plan task update parses clear dependencies");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        crate::args::plan::PlanTaskAction::Update {
                            depends_on,
                            clear_dependencies,
                            ..
                        },
                },
        } => {
            assert!(depends_on.is_empty());
            assert!(clear_dependencies);
        }
        _ => panic!("expected plan task update"),
    }
}

#[test]
fn plan_task_update_rejects_conflicting_dependency_flags() {
    assert!(parse_plan_cli_result([
        "codeterm",
        "plan",
        "task",
        "update",
        "pl_1",
        "task_1",
        "--depends-on",
        "task_a",
        "--clear-dependencies",
    ])
    .is_err());
}

#[test]
fn plan_task_link_and_relink_parse() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "task",
        "link",
        "pl_1",
        "task_1",
        "--sections",
        "backend",
    ])
    .expect("plan task link parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Link { sections, .. },
                },
        } => assert_eq!(sections, "backend"),
        _ => panic!("expected plan task link"),
    }

    let parsed = parse_plan_cli_result(["codeterm", "plan", "task", "relink", "pl_1", "--apply"])
        .expect("plan task relink parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Relink { apply, .. },
                },
        } => assert!(apply),
        _ => panic!("expected plan task relink"),
    }

    // The top-level `plan relink` alias was removed in favour of `plan task relink`.
    assert!(parse_plan_cli_result(["codeterm", "plan", "relink", "pl_1", "--apply"]).is_err());
}

#[test]
fn plan_task_list_parses_plan_and_status() {
    let parsed = parse_plan_cli_result([
        "codeterm", "plan", "task", "list", "pl_1", "--status", "blocked",
    ])
    .expect("plan task list parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        crate::args::plan::PlanTaskAction::List {
                            plan_id,
                            status,
                            all,
                        },
                },
        } => {
            assert_eq!(plan_id.as_deref(), Some("pl_1"));
            assert_eq!(status.as_deref(), Some("blocked"));
            assert!(!all);
        }
        _ => panic!("expected plan task list"),
    }
}

#[test]
fn plan_task_list_parses_all() {
    let parsed = parse_plan_cli_result(["codeterm", "plan", "task", "list", "pl_1", "--all"])
        .expect("plan task list --all parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::List { all, .. },
                },
        } => assert!(all),
        _ => panic!("expected plan task list"),
    }
}

#[test]
fn plan_in_progress_parses_plan_all_and_json() {
    let parsed = parse_plan_cli_result(["codeterm", "plan", "in-progress", "pl_1"])
        .expect("plan in-progress parses");
    match parsed.command {
        Commands::Plan {
            action: PlanAction::InProgress { plan_id, all },
        } => {
            assert_eq!(plan_id.as_deref(), Some("pl_1"));
            assert!(!all);
        }
        _ => panic!("expected plan in-progress"),
    }

    let parsed = parse_plan_cli_result(["codeterm", "--json", "plan", "in-progress", "--all"])
        .expect("plan in-progress --all parses");
    assert!(parsed.json);
    match parsed.command {
        Commands::Plan {
            action: PlanAction::InProgress { plan_id, all },
        } => {
            assert_eq!(plan_id.as_deref(), None);
            assert!(all);
        }
        _ => panic!("expected plan in-progress --all"),
    }

    let err = parse_plan_cli_result(["codeterm", "plan", "in-progress", "pl_1", "--all"]);
    assert!(err.is_err(), "plan id and --all are mutually exclusive");
}

#[test]
fn plan_task_show_parses_plan_and_task_positionals() {
    let parsed = parse_plan_cli_result(["codeterm", "plan", "task", "show", "pl_1", "task_1"])
        .expect("plan task show parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        crate::args::plan::PlanTaskAction::Show {
                            plan_id,
                            task_id,
                            siblings,
                            all,
                        },
                },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(task_id, "task_1");
            assert!(!siblings);
            assert!(!all);
        }
        _ => panic!("expected plan task show"),
    }
}

#[test]
fn plan_task_add_parses_quality_fields() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Quality lane",
        "--criterion",
        "criteria persist",
        "--criterion",
        "task show surfaces them",
        "--test-plan",
        "cargo test service::plan::tests::task_quality_fields_persist_and_reload",
        "--qa-plan",
        "cargo test independent_qa_oracle",
        "--correctness",
        "round-trip unchanged",
    ])
    .expect("plan task add parses quality fields");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        crate::args::plan::PlanTaskAction::Add {
                            criterion,
                            test_plan,
                            qa_plan,
                            correctness,
                            ..
                        },
                },
        } => {
            assert_eq!(
                criterion,
                vec!["criteria persist", "task show surfaces them"]
            );
            assert_eq!(
                test_plan.as_deref(),
                Some("cargo test service::plan::tests::task_quality_fields_persist_and_reload")
            );
            assert_eq!(qa_plan.as_deref(), Some("cargo test independent_qa_oracle"));
            assert_eq!(correctness.as_deref(), Some("round-trip unchanged"));
        }
        _ => panic!("expected plan task add"),
    }
}

#[test]
fn plan_get_parses_sections_lookup() {
    let parsed = parse_plan_cli_result(["codeterm", "plan", "get", "pl_1", "--sections"])
        .expect("plan get --sections parses");
    match parsed.command {
        Commands::Plan {
            action: PlanAction::Get { plan_id, sections },
        } => {
            assert_eq!(plan_id.as_deref(), Some("pl_1"));
            assert!(sections);
        }
        _ => panic!("expected plan get"),
    }
}

#[test]
fn orchestrator_heartbeat_commands_parse() {
    let parsed = OrchestratorOnly::try_parse_from([
        "orchestrator",
        "learn",
        "resolve heartbeat at send time",
        "--plan",
        "pl_1",
        "--tag",
        "heartbeat",
    ])
    .expect("orchestrator learn parses");
    match parsed.action {
        crate::args::orchestrator::OrchestratorAction::Learn { text, plan, tag } => {
            assert_eq!(text, "resolve heartbeat at send time");
            assert_eq!(plan.as_deref(), Some("pl_1"));
            assert_eq!(tag.as_deref(), Some("heartbeat"));
        }
        _ => panic!("expected orchestrator learn"),
    }

    let parsed =
        OrchestratorOnly::try_parse_from(["orchestrator", "heartbeat", "show", "--plan", "pl_1"])
            .expect("orchestrator heartbeat show parses");
    match parsed.action {
        crate::args::orchestrator::OrchestratorAction::Heartbeat {
            action: crate::args::orchestrator::OrchestratorHeartbeatAction::Show { plan },
        } => assert_eq!(plan.as_deref(), Some("pl_1")),
        _ => panic!("expected orchestrator heartbeat show"),
    }
}

#[test]
fn plan_goal_subcommands_parse() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "goal",
        "set",
        "pl_1",
        "--vector",
        "Ship foundation",
        "--vision-file",
        "goal.md",
        "--constraint",
        "backend only",
        "--postulate",
        "reread plan",
    ])
    .expect("plan goal set parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Goal {
                    action:
                        crate::args::plan::PlanGoalAction::Set {
                            plan_id,
                            vector,
                            vision_file,
                            constraint,
                            postulate,
                            ..
                        },
                },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(vector, "Ship foundation");
            assert_eq!(vision_file.as_deref(), Some("goal.md"));
            assert_eq!(constraint, vec!["backend only"]);
            assert_eq!(postulate, vec!["reread plan"]);
        }
        _ => panic!("expected plan goal set"),
    }

    let parsed =
        parse_plan_cli_result(["codeterm", "plan", "goal", "show", "pl_1"]).expect("show parses");
    assert!(matches!(
        parsed.command,
        Commands::Plan {
            action:
                PlanAction::Goal {
                    action: crate::args::plan::PlanGoalAction::Show { plan_id },
                },
        } if plan_id == "pl_1"
    ));

    assert!(parse_plan_cli_result(["codeterm", "plan", "goal", "get", "pl_1"]).is_err());
}

#[test]
fn plan_facets_set_parses() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "facets",
        "set",
        "pl_1",
        "--facets",
        r#"[{"key":"area","color_lane":true,"values":[{"v":"api"}]}]"#,
    ])
    .expect("plan facets set parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Facets {
                    action: crate::args::plan::PlanFacetsAction::Set { plan_id, facets },
                },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert!(facets.contains(r#""key":"area""#));
        }
        _ => panic!("expected plan facets set"),
    }
}

#[test]
fn plan_handoff_subcommands_parse() {
    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "handoff",
        "create",
        "pl_1",
        "--mode",
        "new",
        "--task",
        "backend",
        "--goal",
        "Resume backend handoff",
        "--next",
        "run handoff show",
        "--rule",
        "never reseed parent",
        "--decision",
        "choose ack source",
        "--from-file",
        "handoff.md",
    ])
    .expect("plan handoff create parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Handoff {
                    action:
                        PlanHandoffAction::Create {
                            plan_id,
                            mode,
                            task,
                            goal,
                            next_actions,
                            foundational_rules,
                            open_decisions,
                            from_file,
                            stdin,
                        },
                },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(mode, "new");
            assert_eq!(task.as_deref(), Some("backend"));
            assert_eq!(goal.as_deref(), Some("Resume backend handoff"));
            assert_eq!(next_actions, vec!["run handoff show"]);
            assert_eq!(foundational_rules, vec!["never reseed parent"]);
            assert_eq!(open_decisions, vec!["choose ack source"]);
            assert_eq!(from_file.as_deref(), Some("handoff.md"));
            assert!(!stdin);
        }
        _ => panic!("expected plan handoff create"),
    }

    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "handoff",
        "show",
        "pl_1",
        "--seq",
        "3",
        "--markdown",
    ])
    .expect("plan handoff show parses with seq");
    assert!(matches!(
        parsed.command,
        Commands::Plan {
            action:
                PlanAction::Handoff {
                    action:
                        PlanHandoffAction::Show {
                            plan_id,
                            seq,
                            markdown: true,
                            ..
                        },
                },
        } if plan_id == "pl_1" && seq.as_deref() == Some("3")
    ));

    let parsed = parse_plan_cli_result([
        "codeterm",
        "plan",
        "handoff",
        "show",
        "pl_1",
        "--latest",
        "--markdown",
    ])
    .expect("plan handoff show parses");
    assert!(matches!(
        parsed.command,
        Commands::Plan {
            action:
                PlanAction::Handoff {
                    action:
                        PlanHandoffAction::Show {
                            plan_id,
                            latest: true,
                            markdown: true,
                            seq: None,
                            ..
                        },
                },
        } if plan_id == "pl_1"
    ));

    let parsed = parse_plan_cli_result(["codeterm", "plan", "handoff", "list", "pl_1"])
        .expect("plan handoff list parses");
    assert!(matches!(
        parsed.command,
        Commands::Plan {
            action:
                PlanAction::Handoff {
                    action: PlanHandoffAction::List { plan_id },
                },
        } if plan_id == "pl_1"
    ));
}
