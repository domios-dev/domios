//! `--personality` is a creation contract: a task cannot be created without an
//! explicit one-word executor persona. `plan task update` still sets it on
//! existing tasks, so legacy tasks without one stay repairable.

use clap::Parser;

use crate::{Cli, Commands, PlanAction};

#[test]
fn personality_required_add_refuses_a_task_without_one_and_names_the_flag() {
    let error = Cli::try_parse_from([
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
    .expect_err("plan task add must not create a task with no executor persona");

    let rendered = error.to_string();
    assert!(rendered.contains("--personality"), "{rendered}");
    assert!(
        rendered.to_lowercase().contains("persona"),
        "the refusal must name the expected shape: {rendered}"
    );
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
