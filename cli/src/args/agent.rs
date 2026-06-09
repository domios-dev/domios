use clap::Subcommand;

#[derive(Subcommand)]
pub enum AgentAction {
    /// Mark a pane as orchestrator (top-level, can spawn leads/workers)
    #[command(alias = "set-conductor")]
    SetOrchestrator {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
    },

    /// List agent panes and the hierarchy (same view as `pane list --agents`)
    #[command(alias = "hierarchy")]
    List,
    /// Link a pane as worker under a parent
    Link {
        /// Parent pane ID (orchestrator or lead)
        #[arg(long)]
        parent: String,
        /// Pane ID to link (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Remove a pane's role (also orphans children)
    Unlink {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Toggle whether a pane can spawn children
    AllowSpawn {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
        /// Enable or disable spawning
        #[arg(long, default_value = "true")]
        enable: bool,
    },
    /// Send a report to your parent in the hierarchy
    Report {
        /// Report message (also accepts --message alias)
        message: String,
        /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Read reports (for orchestrator panes)
    Reports {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
        /// Clear reports after reading
        #[arg(long)]
        clear: bool,
        /// Show only the N most recent reports (default: all)
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Compact agent context (sends /compact to Claude)
    Compact {
        /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Restart agent (gracefully /exit + auto-resume to pick up settings or recover)
    Restart {
        /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long)]
        pane: Option<String>,
        /// Skip the graceful exit and force-kill instead. Use only when the agent is hung.
        #[arg(long)]
        force: bool,
        /// Wait this long for graceful exit before force-killing (seconds)
        #[arg(long, default_value = "10")]
        grace_secs: u64,
    },
    /// Switch agent model. Claude only — Codex /model is an interactive picker, respawn with `agent spawn codex --model X` instead.
    Model {
        /// Model name (Claude: sonnet, haiku, opus). Run `agent models claude` to list.
        name: String,
        /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// List available agent providers (claude, codex, ...) with their
    /// display names, default models, and capability flags.
    Providers,
    /// List available models for a specific provider.
    Models {
        /// Provider id (e.g. "claude"). Required.
        provider: String,
    },
    /// Spawn a new agent (creates tab in workspace, linked in hierarchy)
    Spawn {
        /// Agent provider id. Run `codeterm agent providers` to list registered providers (e.g. claude, codex, opencode). Default: claude.
        #[arg(default_value = "claude")]
        agent: String,
        /// Tab title
        #[arg(long)]
        title: Option<String>,
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Role: Orchestrator, Worker
        #[arg(long)]
        role: Option<String>,
        /// Parent pane ID (for Workers)
        #[arg(long)]
        parent: Option<String>,
        /// Allow spawning children
        #[arg(long)]
        can_spawn: bool,
        /// Task to send after agent starts
        #[arg(long)]
        task: Option<String>,
        /// Extra args for the agent
        #[arg(long, num_args = 1..)]
        args: Option<Vec<String>>,
        /// Target workspace ID (default: parent's workspace or active workspace)
        #[arg(long)]
        workspace: Option<String>,
        /// Model selector (e.g. "github-copilot/claude-haiku-4.5", "gpt-5"). The
        /// provider knows whether to emit `-m X` or `--model X` — caller passes
        /// the model string opaque. Run `codeterm agent models <provider>` to
        /// see what's available.
        #[arg(long)]
        model: Option<String>,
    },
}
