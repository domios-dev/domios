use super::Cli;
use crate::args::cli::Commands;
use crate::args::plan::{PlanAction, PlanTaskAction};
use clap::Parser;

#[test]
fn task_add_accepts_explicit_personality() {
    let cli = Cli::try_parse_from([
        "domios",
        "plan",
        "task",
        "add",
        "pl_fixture",
        "--name",
        "Prism runtime task",
        "--personality",
        "Prism",
        "--executor-role",
        "Engineer",
        "--role-guidance",
        "Own the fixture",
    ])
    .unwrap();
    let Commands::Plan {
        action:
            PlanAction::Task {
                action:
                    PlanTaskAction::Add {
                        name, personality, ..
                    },
            },
    } = cli.command
    else {
        panic!("expected plan task add");
    };
    assert_eq!(name, "Prism runtime task");
    assert_eq!(personality, "Prism");
}
