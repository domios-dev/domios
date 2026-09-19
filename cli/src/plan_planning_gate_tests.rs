use clap::Parser;

use crate::args::plan::{PlanAction, PlanElementAction, PlanNoteAction, PlanTaskAction};
use crate::{Cli, Commands};

#[test]
fn plan_mode_commands_parse() {
    for args in [
        ["codeterm", "plan", "approve", "pl_1"].as_slice(),
        ["codeterm", "plan", "unapprove", "pl_1"].as_slice(),
        ["codeterm", "plan", "mode", "pl_1", "--set", "execution"].as_slice(),
        [
            "codeterm",
            "plan",
            "pause",
            "pl_1",
            "--reason",
            "provider quota exhausted",
        ]
        .as_slice(),
        ["codeterm", "plan", "resume", "pl_1"].as_slice(),
    ] {
        Cli::try_parse_from(args).expect("plan mode command parses");
    }
}

#[test]
fn plan_task_add_force_and_rm_all_parse() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Draft",
        "--executor-role",
        "rust-backend-dev",
        "--personality",
        "Ada",
        "--role-guidance",
        "focused worker",
        "--force",
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
fn plan_adopt_and_move_parse() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "adopt",
        "/tmp/source/.codeterm/plans/active/pl_1",
        "--project",
        "/tmp/repo",
        "--dry-run",
        "--overwrite",
    ])
    .expect("plan adopt parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Adopt {
                    source,
                    project,
                    dry_run,
                    overwrite,
                },
        } => {
            assert_eq!(source, "/tmp/source/.codeterm/plans/active/pl_1");
            assert_eq!(project, "/tmp/repo");
            assert!(dry_run);
            assert!(overwrite);
        }
        _ => panic!("expected plan adopt"),
    }

    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "move",
        "pl_1",
        "--from",
        "/tmp/source",
        "--to",
        "/tmp/repo",
    ])
    .expect("plan move parses");
    match parsed.command {
        Commands::Plan {
            action: PlanAction::Move {
                plan_id, from, to, ..
            },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(from, "/tmp/source");
            assert_eq!(to, "/tmp/repo");
        }
        _ => panic!("expected plan move"),
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
