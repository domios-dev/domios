use super::Cli;
use crate::args::cli::Commands;
use crate::args::plan::{PlanAction, PlanTaskAction};
use clap::Parser;

#[test]
fn task_add_omits_personality_when_the_task_name_supplies_it() {
    let cli = Cli::try_parse_from([
        "domios",
        "plan",
        "task",
        "add",
        "pl_fixture",
        "--name",
        "Prism runtime task",
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
    assert!(personality.is_empty());
}
