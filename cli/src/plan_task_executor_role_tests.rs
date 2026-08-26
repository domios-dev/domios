//! `executor_role` is a contract, not a custom: a task cannot be created without
//! naming who executes it, and no later path can empty the field.

use clap::Parser;

use crate::{Cli, Commands, PlanAction};

#[test]
fn executor_role_required_add_refuses_a_task_without_a_role() {
    let parsed = Cli::try_parse_from([
        "codeterm", "plan", "task", "add", "pl_1", "--name", "Roleless lane",
    ]);
    assert!(
        parsed.is_err(),
        "plan task add must not create a task with nobody named to execute it"
    );
}

// Presence, not vocabulary. This test exists so a future validator, allowlist,
// minimum length or banned-word list cannot be added quietly: any lexical floor
// is satisfiable by a longer shrug, and the meaningful floor -- whether the role
// would change how the Orchestrator launches the task -- is not machine-checkable.
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
