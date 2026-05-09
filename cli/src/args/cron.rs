use clap::Subcommand;

#[derive(Subcommand)]
pub enum CronAction {
    /// List cron jobs
    List {
        /// Filter by workspace ID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Create a cron job
    Create {
        /// Cron job name
        name: String,
        /// Cron schedule (5-field: "*/30 * * * *") or oneshot time ("in 5m", ISO8601)
        #[arg(long)]
        schedule: String,
        /// Task prompt for the agent
        #[arg(long)]
        task: String,
        /// Workspace ID (default: active)
        #[arg(long)]
        workspace: Option<String>,
        /// Agent to spawn (default: claude)
        #[arg(long, default_value = "claude")]
        agent: String,
        /// Model within the agent provider (e.g. "opus", "haiku",
        /// "gpt-5.4-mini"). Leave unset to use the provider default.
        #[arg(long)]
        model: Option<String>,
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Pane ID to receive reports from cron agent (default: $CODETERM_PANE_ID)
        #[arg(long, env = "CODETERM_PANE_ID")]
        creator_pane: Option<String>,
        /// Oneshot: fire once at scheduled time, then auto-delete
        #[arg(long)]
        once: bool,
        /// Max jitter in milliseconds added to each scheduled fire. 0 = off.
        /// Omit for default (10% of interval, clamped to [1m, 30m]).
        #[arg(long)]
        jitter_max_ms: Option<u64>,
        /// IANA timezone for the schedule (e.g. "Europe/Riga"). Default: UTC.
        #[arg(long)]
        tz: Option<String>,
        /// Per-cron run timeout (seconds). Overrides global memory.worker_timeout_seconds.
        /// Set high (e.g. 1800) for long-running tasks like memory dreaming.
        #[arg(long)]
        timeout_secs: Option<u64>,
    },
    /// Edit a cron job
    Edit {
        /// Cron job ID
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        schedule: Option<String>,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Agent provider override (e.g. "claude", "codex")
        #[arg(long)]
        agent: Option<String>,
        /// Model within the agent provider. Pass empty string to clear.
        #[arg(long)]
        model: Option<String>,
        /// Max jitter in milliseconds added to each scheduled fire. Set
        /// 0 to disable jitter. Omit to leave unchanged.
        #[arg(long)]
        jitter_max_ms: Option<u64>,
        /// IANA timezone (e.g. "Europe/Riga"). Pass empty string to clear back to UTC.
        #[arg(long)]
        tz: Option<String>,
        /// Per-cron run timeout (seconds). 0 clears the override (back to global default).
        #[arg(long)]
        timeout_secs: Option<u64>,
    },
    /// Delete a cron job
    Delete {
        /// Cron job ID
        id: String,
    },
    /// Enable a cron job
    Enable {
        /// Cron job ID
        id: String,
    },
    /// Disable a cron job
    Disable {
        /// Cron job ID
        id: String,
    },
    /// Stop any currently-running executions of a cron job without
    /// deleting the job definition. Future scheduled fires still run.
    Cancel {
        /// Cron job ID
        id: String,
    },
    /// Manually trigger a cron job now
    Trigger {
        /// Cron job ID
        id: String,
    },
    /// Show run history
    History {
        /// Cron job ID
        id: String,
    },
    /// Mark a cron run as complete (called by agent at end of task)
    Complete {
        /// Run ID
        #[arg(long)]
        run_id: String,
        /// Summary of what happened
        #[arg(long)]
        summary: Option<String>,
    },
}
