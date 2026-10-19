use crate::args::cli::Cli;
use clap::Parser;

#[test]
fn worker_spawn_sandbox_is_explicit_and_off_by_default() {
    for mode in [None, Some("read-only"), Some("workspace-write")] {
        let mut args = vec![
            "codeterm",
            "agent",
            "spawn",
            "codex",
            "--role",
            "Worker",
            "--plan",
            "pl",
            "--execution-intent",
            "read_only",
        ];
        if let Some(mode) = mode {
            args.extend(["--sandbox", mode]);
        }
        let parsed = Cli::try_parse_from(args).unwrap();
        let crate::Commands::Agent {
            action: super::AgentAction::Spawn { sandbox, .. },
        } = parsed.command
        else {
            panic!("spawn");
        };
        assert_eq!(sandbox.as_deref(), mode);
    }
}

#[test]
fn dispatch_sandbox_is_explicit_and_off_by_default() {
    for mode in [None, Some("read-only"), Some("workspace-write")] {
        let mut args = vec![
            "codeterm",
            "plan",
            "task",
            "dispatch",
            "pl",
            "task",
            "--execution-intent",
            "mutating",
        ];
        if let Some(mode) = mode {
            args.extend(["--sandbox", mode]);
        }
        let parsed = Cli::try_parse_from(args).unwrap();
        let crate::Commands::Plan {
            action:
                crate::PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Dispatch { sandbox, .. },
                },
        } = parsed.command
        else {
            panic!("dispatch");
        };
        assert_eq!(sandbox.as_deref(), mode);
    }
}
