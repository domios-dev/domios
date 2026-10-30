//! `executor_role` is a contract, not a custom: a task cannot be created without
//! naming who executes it, and no later path can empty the field.

use clap::Parser;

use crate::{Cli, Commands, PlanAction};

#[test]
fn executor_role_required_add_reaches_the_aggregate_daemon_validator() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Roleless lane",
    ])
    .expect("missing required draft fields are reported together by the daemon");
    assert!(matches!(parsed.command, Commands::Plan {
        action: PlanAction::Task { action: crate::args::plan::PlanTaskAction::Add { executor_role, .. } }
    } if executor_role.is_empty()));
}

#[test]
fn executor_role_required_presence_only_accepts_a_short_unimaginative_role() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Short role lane",
        "--executor-role",
        "dev",
        "--personality",
        "Ada",
        "--role-guidance",
        "x",
    ])
    .expect("a short, unimaginative role is a present role");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Add { executor_role, .. },
                },
        } => assert_eq!(executor_role, "dev"),
        _ => panic!("expected plan task add"),
    }
}
