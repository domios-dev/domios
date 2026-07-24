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
    /// Set the heartbeat focus, active lanes, and rules carried into auto-reinjection
    Set {
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        focus: Option<String>,
        #[arg(long = "lane", allow_hyphen_values = true)]
        lanes: Vec<String>,
        #[arg(long = "rule", allow_hyphen_values = true)]
        rules: Vec<String>,
        #[arg(long)]
        clear_lanes: bool,
        #[arg(long)]
        clear_rules: bool,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::args::cli::{Cli, Commands};

    use super::{OrchestratorAction, OrchestratorHeartbeatAction};

    #[test]
    fn status_parses_plan_and_pane() {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "orchestrator",
            "status",
            "--plan",
            "pl_1",
            "--tab",
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
            "--tab",
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
            "--tab",
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
    fn heartbeat_set_parses_clear_lanes_with_lane_for_cli_validation() {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "orchestrator",
            "heartbeat",
            "set",
            "--plan",
            "pl_1",
            "--clear-lanes",
            "--lane",
            "L2",
        ])
        .expect("heartbeat set parses");
        match parsed.command {
            Commands::Orchestrator {
                action:
                    OrchestratorAction::Heartbeat {
                        action:
                            OrchestratorHeartbeatAction::Set {
                                plan,
                                lanes,
                                clear_lanes,
                                ..
                            },
                    },
            } => {
                assert_eq!(plan.as_deref(), Some("pl_1"));
                assert_eq!(lanes, vec!["L2"]);
                assert!(clear_lanes);
            }
            _ => panic!("expected heartbeat set"),
        }
    }
}
