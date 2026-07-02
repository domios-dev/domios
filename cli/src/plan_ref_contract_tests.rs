use crate::args::cli::{Cli, Commands};
use crate::args::plan::{PlanAction, PlanTaskAction};
use clap::Parser;

#[test]
fn plan_task_update_accepts_task_ref_alone() {
    // Parent-free: one positional is the task ref; plan is inferred daemon-side.
    let parsed = Cli::try_parse_from([
        "codeterm", "plan", "task", "update", "auth-login", "--status", "done",
    ])
    .expect("plan task update parses with task ref alone");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        PlanTaskAction::Update {
                            plan_id,
                            task_id,
                            status,
                            ..
                        },
                },
        } => {
            assert_eq!(plan_id, "auth-login");
            assert_eq!(task_id, None);
            assert_eq!(status.as_deref(), Some("done"));
        }
        _ => panic!("expected plan task update"),
    }
}

#[test]
fn plan_task_update_accepts_plan_and_task() {
    let parsed = Cli::try_parse_from([
        "codeterm", "plan", "task", "update", "pl_1", "task_1", "--status", "done",
    ])
    .expect("plan task update parses plan + task");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: PlanTaskAction::Update { plan_id, task_id, .. },
                },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(task_id.as_deref(), Some("task_1"));
        }
        _ => panic!("expected plan task update"),
    }
}

#[test]
fn plan_task_rm_accepts_task_ref_alone() {
    let parsed = Cli::try_parse_from(["codeterm", "plan", "task", "rm", "auth-login"])
        .expect("plan task rm parses with task ref alone");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: PlanTaskAction::Rm { plan_id, task_id, all, .. },
                },
        } => {
            assert_eq!(plan_id, "auth-login");
            assert_eq!(task_id, None);
            assert!(!all);
        }
        _ => panic!("expected plan task rm"),
    }
}
