use clap::Parser;

use crate::args::plan::{PlanAction, PlanElementAction, PlanNoteAction, PlanTaskAction};
use crate::{Cli, Commands};

#[test]
fn plan_mode_commands_parse() {
    for args in [
        ["codeterm", "plan", "approve", "pl_1"].as_slice(),
        ["codeterm", "plan", "unapprove", "pl_1"].as_slice(),
        ["codeterm", "plan", "mode", "pl_1", "--set", "execution"].as_slice(),
    ] {
        Cli::try_parse_from(args).expect("plan mode command parses");
    }
}

#[test]
fn plan_task_add_force_and_rm_all_parse() {
    let parsed = Cli::try_parse_from([
        "codeterm", "plan", "task", "add", "pl_1", "--name", "Draft", "--force",
    ])
    .expect("plan task add --force parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: PlanTaskAction::Add { force, .. },
                },
        } => assert!(force),
        _ => panic!("expected plan task add"),
    }

    let parsed = Cli::try_parse_from(["codeterm", "plan", "task", "rm", "pl_1", "--all", "--yes"])
        .expect("plan task rm --all --yes parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: PlanTaskAction::Rm { all, yes, .. },
                },
        } => {
            assert!(all);
            assert!(yes);
        }
        _ => panic!("expected plan task rm"),
    }
}

#[test]
fn plan_note_and_element_mutations_parse() {
    let parsed = Cli::try_parse_from([
        "codeterm", "plan", "note", "update", "pl_1", "el_1", "--text", "new",
    ])
    .expect("plan note update parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Note {
                    action: PlanNoteAction::Update { note_id, .. },
                },
        } => assert_eq!(note_id, "el_1"),
        _ => panic!("expected plan note update"),
    }

    let parsed = Cli::try_parse_from(["codeterm", "plan", "element", "rm", "pl_1", "el_1"])
        .expect("plan element rm parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Element {
                    action: PlanElementAction::Rm { element_id, .. },
                },
        } => assert_eq!(element_id, "el_1"),
        _ => panic!("expected plan element rm"),
    }
}
