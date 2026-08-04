use crate::args::plan::{PlanAction, PlanTaskAction};
use crate::{Cli, Commands};
use clap::Parser;

#[test]
fn plan_task_attempts_and_respawn_parse() {
    let parsed = Cli::try_parse_from(["codeterm", "plan", "task", "attempts", "pl_1", "task_1"])
        .expect("plan task attempts parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: PlanTaskAction::Attempts { plan_id, task_id },
                },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(task_id, "task_1");
        }
        _ => panic!("expected plan task attempts"),
    }

    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "respawn",
        "pl_1",
        "task_1",
        "--attempt",
        "attempt_1",
        "--provider",
        "codex",
        "--model",
        "gpt-5.5",
        "--parent",
        "tab_parent",
        "--workspace",
        "ws_1",
    ])
    .expect("plan task respawn parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        PlanTaskAction::Respawn {
                            attempt,
                            provider,
                            model,
                            parent,
                            workspace,
                            ..
                        },
                },
        } => {
            assert_eq!(attempt, "attempt_1");
            assert_eq!(provider.as_deref(), Some("codex"));
            assert_eq!(model.as_deref(), Some("gpt-5.5"));
            assert_eq!(parent.as_deref(), Some("tab_parent"));
            assert_eq!(workspace.as_deref(), Some("ws_1"));
        }
        _ => panic!("expected plan task respawn"),
    }
}
