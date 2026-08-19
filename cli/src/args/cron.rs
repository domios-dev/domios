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
        #[arg(long, allow_hyphen_values = true)]
        task: String,
        /// Workspace ID. Defaults to the calling tab's own workspace — pass this
        /// only to schedule the job somewhere other than where you are.
        #[arg(long)]
        workspace: Option<String>,
        /// Attach this job to a Team. The calling Tab must belong to it.
        #[arg(long, value_name = "TEAM_ID")]
        team: Option<String>,
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
        /// Mem skill slug (`codeterm mem skill new <slug>`) to resolve and inject
        /// into the task brief at fire time as an explicit, Read-able path.
        #[arg(long)]
        skill: Option<String>,
        /// Description
        #[arg(long, allow_hyphen_values = true)]
        description: Option<String>,
        /// Tab ID to receive reports. Omit to keep runs private to the cron.
        #[arg(long = "tab", value_name = "TAB")]
        report_tab: Option<String>,
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
        #[arg(long, allow_hyphen_values = true)]
        task: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        /// Mem skill slug to resolve and inject into the task brief at fire
        /// time. Pass an empty string to clear.
        #[arg(long)]
        skill: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
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

/// Single source of truth for the `cron.create` request body, shared by the
/// local socket path (`cmd/cron.rs`) and the mesh HTTP path
/// (`mesh_cli/api_map.rs`). Both transports deserialize into the server's
/// `CronCreateParams`, so the JSON shape must match in exactly one place —
/// add a new cron field here and both paths pick it up. `workspace_id` is
/// passed already-resolved because resolution differs per transport (local
/// falls back to the active workspace; mesh requires it explicit).
#[allow(clippy::too_many_arguments)]
pub fn cron_create_body(
    name: &str,
    schedule: &str,
    task: &str,
    workspace_id: &str,
    team_id: Option<&str>,
    agent: &str,
    model: Option<&str>,
    cwd: Option<&str>,
    description: Option<&str>,
    creator_tab: Option<&str>,
    report_tab: Option<&str>,
    once: bool,
    jitter_max_ms: Option<u64>,
    tz: Option<&str>,
    timeout_secs: Option<u64>,
    skill: Option<&str>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "name": name,
        "schedule": schedule,
        "task": task,
        "workspace_id": workspace_id,
        "agent": agent,
    });
    if let Some(m) = model {
        body["model"] = serde_json::json!(m);
    }
    if let Some(c) = cwd {
        body["cwd"] = serde_json::json!(c);
    }
    if let Some(s) = skill {
        body["skill"] = serde_json::json!(s);
    }
    if let Some(d) = description {
        body["description"] = serde_json::json!(d);
    }
    if let Some(cp) = creator_tab {
        body["creator_tab_id"] = serde_json::json!(cp);
    }
    if let Some(team) = team_id {
        body["team_id"] = serde_json::json!(team);
    }
    if let Some(target) = report_tab {
        body["report_target_tab_id"] = serde_json::json!(target);
    }
    if once {
        body["oneshot"] = serde_json::json!(true);
    }
    if let Some(j) = jitter_max_ms {
        body["jitter_max_ms"] = serde_json::json!(j);
    }
    if let Some(z) = tz {
        body["tz"] = serde_json::json!(z);
    }
    if let Some(t) = timeout_secs {
        body["timeout_seconds"] = serde_json::json!(t);
    }
    body
}
