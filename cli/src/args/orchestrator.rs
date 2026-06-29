use clap::Subcommand;

#[derive(Subcommand)]
pub enum OrchestratorAction {
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
    /// Create an orchestrator handoff and optionally reseed this pane
    Handoff {
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
        #[arg(long, env = "CODETERM_TASK_ID")]
        task: Option<String>,
        #[arg(long, env = "CODETERM_PANE_ID")]
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
