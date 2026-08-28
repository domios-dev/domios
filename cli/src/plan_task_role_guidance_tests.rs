use clap::Parser;

use crate::{Cli, Commands, PlanAction};

#[test]
fn role_guidance_contract_add_refuses_absence_and_update_refuses_blank() {
    let missing = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Unconditioned lane",
        "--executor-role",
        "dev",
    ]);
    assert!(missing.is_err());

    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "update",
        "pl_1",
        "task_1",
        "--role-guidance",
        "",
    ])
    .expect("update syntax reaches the shared value contract");
    let guidance = match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Update { role_guidance, .. },
                },
        } => role_guidance,
        _ => panic!("expected plan task update"),
    };
    assert!(crate::cmd::plan::resolve_role_guidance(guidance, None).is_err());
}

#[test]
fn role_guidance_contract_presence_only_accepts_short_unimaginative_guidance() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Thin guidance lane",
        "--executor-role",
        "dev",
        "--role-guidance",
        "do it",
    ])
    .expect("presence is sufficient; prose quality is not machine-validated");
    let guidance = match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Add { role_guidance, .. },
                },
        } => role_guidance,
        _ => panic!("expected plan task add"),
    };
    assert_eq!(
        crate::cmd::plan::resolve_role_guidance(guidance, None)
            .unwrap()
            .as_deref(),
        Some("do it")
    );
}

#[test]
fn plan_task_add_and_update_parse_role_guidance_flags() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Planner lane",
        "--executor-role",
        "docs-sync",
        "--role-guidance",
        "Act as planner",
        "--role-guidance-file",
        "guidance.md",
    ])
    .expect("plan task add parses role guidance");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        crate::args::plan::PlanTaskAction::Add {
                            role_guidance,
                            role_guidance_file,
                            ..
                        },
                },
        } => {
            assert_eq!(role_guidance.as_deref(), Some("Act as planner"));
            assert_eq!(role_guidance_file.as_deref(), Some("guidance.md"));
        }
        _ => panic!("expected plan task add"),
    }

    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "update",
        "pl_1",
        "task_1",
        "--role-guidance",
        "Review only",
    ])
    .expect("plan task update parses role guidance");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Update { role_guidance, .. },
                },
        } => assert_eq!(role_guidance.as_deref(), Some("Review only")),
        _ => panic!("expected plan task update"),
    }
}

#[test]
fn plan_task_add_and_update_parse_executor_role_flags() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Executor lane",
        "--executor-role",
        "rust-backend-dev",
        "--role-guidance",
        "focused worker",
    ])
    .expect("plan task add parses executor role");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Add { executor_role, .. },
                },
        } => assert_eq!(executor_role, "rust-backend-dev"),
        _ => panic!("expected plan task add"),
    }

    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "update",
        "pl_1",
        "task_1",
        "--executor-role",
        "frontend-dev",
    ])
    .expect("plan task update parses executor role");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Update { executor_role, .. },
                },
        } => assert_eq!(executor_role.as_deref(), Some("frontend-dev")),
        _ => panic!("expected plan task update"),
    }
}
