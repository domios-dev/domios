use clap::Parser;

use crate::{Cli, Commands, PlanAction};

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
