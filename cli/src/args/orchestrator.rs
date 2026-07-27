use clap::Subcommand;

#[derive(Subcommand)]
pub enum OrchestratorAction {
    /// Show a compact orchestration summary
    Status {
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
        #[arg(long = "tab", value_name = "TAB", env = "CODETERM_PANE_ID")]
        pane: Option<String>,
    },
    /// Show the full orchestration snapshot, including report bodies
    Inspect {
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
        #[arg(long = "tab", value_name = "TAB", env = "CODETERM_PANE_ID")]
        pane: Option<String>,
    },
    /// Append a persistent learning to this orchestrator's heartbeat
    Learn {
        #[arg(allow_hyphen_values = true)]
        text: String,
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
        #[arg(long)]
        tag: Option<String>,
    },
    /// Orchestrator heartbeat helpers
    Heartbeat {
        #[command(subcommand)]
        action: OrchestratorHeartbeatAction,
    },
    /// Run the daemon-side refresh cycle (handoff → compact → re-orient → verify resumption)
    Refresh {
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
        #[arg(long = "tab", value_name = "TAB", env = "CODETERM_PANE_ID")]
        pane: Option<String>,
    },
    /// Create an orchestrator handoff and optionally reseed this tab
    Handoff {
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
        #[arg(long, env = "CODETERM_TASK_ID")]
        task: Option<String>,
        #[arg(long = "tab", value_name = "TAB", env = "CODETERM_PANE_ID")]
        pane: Option<String>,
        #[arg(long)]
        reseed: bool,
        #[arg(long, default_value = "new")]
        mode: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        force: bool,
        #[arg(long, allow_hyphen_values = true)]
        goal: Option<String>,
        #[arg(long = "next", allow_hyphen_values = true)]
        next_actions: Vec<String>,
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum OrchestratorHeartbeatAction {
    /// Show the current heartbeat and learnings
    Show {
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::args::cli::{Cli, Commands};

    use super::OrchestratorAction;

    #[test]
    fn status_parses_plan_and_pane() {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "orchestrator",
            "status",
            "--plan",
            "pl_1",
            "--pane",
            "pane1",
        ])
        .expect("orchestrator status parses");
        match parsed.command {
            Commands::Orchestrator {
                action: OrchestratorAction::Status { plan, pane },
            } => {
                assert_eq!(plan.as_deref(), Some("pl_1"));
                assert_eq!(pane.as_deref(), Some("pane1"));
            }
            _ => panic!("expected orchestrator status"),
        }
    }

    #[test]
    fn inspect_parses_plan_and_pane() {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "orchestrator",
            "inspect",
            "--plan",
            "pl_1",
            "--pane",
            "pane1",
        ])
        .expect("orchestrator inspect parses");
        match parsed.command {
            Commands::Orchestrator {
                action: OrchestratorAction::Inspect { plan, pane },
            } => {
                assert_eq!(plan.as_deref(), Some("pl_1"));
                assert_eq!(pane.as_deref(), Some("pane1"));
            }
            _ => panic!("expected orchestrator inspect"),
        }
    }

    #[test]
    fn refresh_parses_plan_and_pane() {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "orchestrator",
            "refresh",
            "--plan",
            "pl_1",
            "--pane",
            "pane1",
        ])
        .expect("orchestrator refresh parses");
        match parsed.command {
            Commands::Orchestrator {
                action: OrchestratorAction::Refresh { plan, pane },
            } => {
                assert_eq!(plan.as_deref(), Some("pl_1"));
                assert_eq!(pane.as_deref(), Some("pane1"));
            }
            _ => panic!("expected orchestrator refresh"),
        }
    }

    #[test]
    fn heartbeat_set_is_not_a_public_command() {
        assert!(Cli::try_parse_from([
            "codeterm",
            "orchestrator",
            "heartbeat",
            "set",
            "--plan",
            "pl_1",
        ])
        .is_err());
    }
}
