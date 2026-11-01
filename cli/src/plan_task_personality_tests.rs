//! Explicit personas stay validated; omitted personas are derived by the daemon.

use clap::Parser;

use crate::{Cli, Commands, PlanAction};

#[test]
fn ease_task_add_defers_missing_required_fields_to_the_aggregate_validator() {
    let parsed = Cli::try_parse_from(["domios", "plan", "task", "add", "pl_1"]).unwrap();
    assert!(matches!(parsed.command, Commands::Plan {
        action: PlanAction::Task { action: crate::args::plan::PlanTaskAction::Add { name, executor_role, role_guidance: None, .. } }
    } if name.is_empty() && executor_role.is_empty()));
}

#[test]
fn ease_personality_add_allows_omission_for_daemon_derivation() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Nameless lane",
        "--executor-role",
        "dev",
        "--role-guidance",
        "focused worker",
    ])
    .expect("the daemon derives an omitted executor persona");
    assert!(matches!(parsed.command, Commands::Plan {
        action: PlanAction::Task { action: crate::args::plan::PlanTaskAction::Add { personality, .. } }
    } if personality.is_empty()));
}

#[test]
fn personality_required_add_accepts_a_one_word_persona() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Named lane",
        "--executor-role",
        "rust-backend-dev",
        "--personality",
        "Ada",
        "--role-guidance",
        "focused worker",
    ])
    .expect("a one-word persona is a present persona");

    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Add { personality, .. },
                },
        } => assert_eq!(personality, "Ada"),
        _ => panic!("expected plan task add"),
    }
}

#[test]
fn personality_creation_contract_rejects_role_slugs_and_multi_word_names() {
    assert!(codeterm_types::plan::validate_executor_name("rust-backend-dev").is_err());
    assert!(codeterm_types::plan::validate_executor_name("Ada Lovelace").is_err());
    assert!(codeterm_types::plan::validate_executor_name("").is_err());
    assert_eq!(
        codeterm_types::plan::validate_executor_name("Emma").unwrap(),
        "Emma"
    );
}

#[test]
fn personality_update_can_still_set_it_on_an_existing_task() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "update",
        "pl_1",
        "task_1",
        "--personality",
        "Hopper",
    ])
    .expect("update parses personality");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Update { personality, .. },
                },
        } => assert_eq!(personality.as_deref(), Some("Hopper")),
        _ => panic!("expected plan task update"),
    }

    let untouched = Cli::try_parse_from(["codeterm", "plan", "task", "update", "pl_1", "task_1"])
        .expect("an update without --personality stays valid");
    assert!(matches!(
        untouched.command,
        Commands::Plan {
            action: PlanAction::Task {
                action: crate::args::plan::PlanTaskAction::Update {
                    personality: None,
                    ..
                },
            },
        }
    ));
}
