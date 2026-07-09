use clap::Parser;

use crate::{Cli, Commands};

#[test]
fn plan_task_attach_parses() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "attach",
        "pl_1",
        "task_1",
        "evidence.png",
    ])
    .expect("plan task attach parses");
    match parsed.command {
        Commands::Plan {
            action: crate::args::plan::PlanAction::Task { action },
        } => match action {
            crate::args::plan::PlanTaskAction::Attach {
                plan_id,
                task_id,
                file,
            } => {
                assert_eq!(plan_id, "pl_1");
                assert_eq!(task_id, "task_1");
                assert_eq!(file, "evidence.png");
            }
            _ => panic!("expected plan task attach"),
        },
        _ => panic!("expected plan command"),
    }
}

#[test]
fn agent_report_attach_parses_repeated_files() {
    let parsed = Cli::try_parse_from([
        "codeterm", "agent", "report", "done", "--attach", "one.png", "--attach", "two.jpg",
    ])
    .expect("agent report --attach parses");
    match parsed.command {
        Commands::Agent {
            action: crate::args::agent::AgentAction::Report(args),
        } => assert_eq!(args.attachments, vec!["one.png", "two.jpg"]),
        _ => panic!("expected agent report"),
    }
}
