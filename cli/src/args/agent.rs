use super::agent_runtime::{AgentUsageAction, ChildHistoryArgs};
use clap::{Args, Subcommand};

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnLocation {
    Local,
    Cloud,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum SpawnRole {
    #[value(name = "Orchestrator")]
    Orchestrator,
    #[value(name = "Worker")]
    Worker,
    #[value(name = "Manager")]
    Manager,
    #[value(name = "Planner")]
    Planner,
    #[value(name = "Watcher")]
    Watcher,
}

impl SpawnRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpawnRole::Orchestrator => "Orchestrator",
            SpawnRole::Manager => "Manager",
            SpawnRole::Worker => "Worker",
            SpawnRole::Planner => "Manager",
            SpawnRole::Watcher => "Manager",
        }
    }

    pub fn role_profile(&self) -> Option<&'static str> {
        match self {
            SpawnRole::Planner => Some("planner"),
            SpawnRole::Watcher => Some("watcher"),
            _ => None,
        }
    }
}

/// Worker-declared outcome for `agent report --status`. Lowercase value names
/// match the daemon's `ReportStatus` serde representation, so the parsed value
/// is sent over the wire as-is. clap rejects anything outside known outcomes.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum ReportStatusArg {
    Done,
    #[value(name = "in_review")]
    InReview,
    Blocked,
    Partial,
}

impl ReportStatusArg {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReportStatusArg::Done => "done",
            ReportStatusArg::InReview => "in_review",
            ReportStatusArg::Blocked => "blocked",
            ReportStatusArg::Partial => "partial",
        }
    }
}

#[derive(Args, Clone, Debug)]
pub struct ReportArgs {
    /// Report message (also accepts --message alias)
    pub message: Option<String>,
    /// Report message as a flag, useful when paired with --doc
    #[arg(long, alias = "message", allow_hyphen_values = true)]
    pub text: Option<String>,
    /// Milestone kind when writing to --plan/--task
    #[arg(long)]
    pub kind: Option<String>,
    /// Markdown document to append to the report milestone
    #[arg(long, value_name = "PATH")]
    pub doc: Option<String>,
    /// Read the report body from a relative file in the current agent tab's pad
    #[arg(long, value_name = "PATH", conflicts_with_all = ["message", "text", "doc"])]
    pub from_pad: Option<String>,
    /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
    #[arg(long = "tab", value_name = "TAB")]
    pub tab: Option<String>,
    /// Plan id to append this report to (if available)
    #[arg(long, env = "CODETERM_PLAN_ID")]
    pub plan: Option<String>,
    /// Task id to append this report to (if available)
    #[arg(long, env = "CODETERM_TASK_ID")]
    pub task: Option<String>,
    /// Report outcome: in_review | blocked (proposes a task block) | partial | done
    #[arg(long)]
    pub status: Option<ReportStatusArg>,
    /// Git branch the work landed on
    #[arg(long)]
    pub branch: Option<String>,
    /// Commit SHA produced
    #[arg(long)]
    pub sha: Option<String>,
    /// Base commit/ref for changed-file attribution (`base..HEAD` plus dirty overlay)
    #[arg(long, env = "CODETERM_LANE_BASE")]
    pub base: Option<String>,
    /// Changed files (comma-separated or repeat the flag)
    #[arg(long, value_delimiter = ',')]
    pub changed_files: Option<Vec<String>>,
    /// Test command that was run
    #[arg(long)]
    pub test_cmd: Option<String>,
    /// Result of the test command (e.g. "42 passed")
    #[arg(long)]
    pub test_result: Option<String>,
    /// Anything deliberately skipped
    #[arg(long)]
    pub skipped: Option<String>,
    /// Acknowledge unrelated dirty files in the worktree and proceed with the
    /// readiness report. Stride is recorded acknowledging the override.
    #[arg(long)]
    pub acknowledge_dirty: bool,
    /// File to attach to the report milestone; repeat for multiple files
    #[arg(long = "attach", value_name = "PATH")]
    pub attachments: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum AgentAction {
    /// Move an unbound running worker under a live local Team Orchestrator without restarting it
    Reparent {
        tab: String,
        #[arg(long)]
        to: String,
        /// Peer that owns the worker; --to always names an Orchestrator on this machine
        #[arg(long)]
        peer: Option<String>,
    },
    /// Read usage or consume a confirmed reset credit for an exact tab.
    Usage {
        #[command(subcommand)]
        action: AgentUsageAction,
    },
    /// Read one recorded child's transcript under the exact parent binding.
    ChildHistory(ChildHistoryArgs),
    /// Sign in to the tab's runtime provider without a terminal
    Login {
        /// Read the pending sign-in without starting another flow
        #[arg(long, conflicts_with_all = ["device", "api_key_stdin", "cancel"])]
        show: bool,
        #[arg(long, value_name = "TAB")]
        tab: String,
        #[arg(long, conflicts_with_all = ["api_key_stdin", "cancel"])]
        device: bool,
        /// Read the API key from stdin; never supply credentials in argv
        #[arg(long, conflicts_with = "cancel")]
        api_key_stdin: bool,
        #[arg(long)]
        cancel: bool,
    },
    /// Read completed turns from a tool-less agent's exact bound session
    Turns {
        #[arg(long, value_name = "TAB")]
        tab: String,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=50))]
        last: u16,
        #[arg(
            long,
            default_value_t = 0,
            help = "UTF-8 user-record lower bound; use next_offset to continue a bounded window"
        )]
        after: u64,
    },
    /// Promote your own live roleless, team-less agent to Orchestrator (can spawn workers)
    SetOrchestrator {
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },

    /// Link a tab as worker under a parent
    Link {
        /// Parent tab ID (orchestrator)
        #[arg(long = "parent-tab")]
        parent: String,
        /// Tab ID to link (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Remove a tab's role. A final Orchestrator leaves its Team and Plan recoverable
    Unlink {
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Toggle whether a tab can spawn children
    AllowSpawn {
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Enable or disable spawning
        #[arg(long, default_value = "true")]
        enable: bool,
    },
    /// Send a report to your parent in the hierarchy
    Report(ReportArgs),
    /// Acknowledge receipt and ownership of the assigned task
    Ack {
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Plan id (default: $CODETERM_PLAN_ID)
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
        /// Task id (default: $CODETERM_TASK_ID)
        #[arg(long, env = "CODETERM_TASK_ID")]
        task: Option<String>,
    },
    /// Acknowledge that this spawned agent received its first startup turn
    StartupAck,
    /// Cancel automatic cleanup for an inspected startup attempt
    StartupHold {
        /// Target spawned Tab
        #[arg(long = "tab", value_name = "TAB")]
        tab: String,
        /// Launch attempt shown in the startup-overdue report
        #[arg(long)]
        attempt: String,
    },
    /// List an orchestrator's workers from the durable ledger (survives daemon
    /// restart): branch / model / status / last-seen, one row per worker.
    Ledger {
        /// Orchestrator tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Compact agent context (sends /compact to Claude)
    Compact {
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Restart a live agent, or resume an exited provider's verified session in its shell
    Restart {
        /// Tab ID (defaults to $CODETERM_TAB_ID locally; required with --mesh)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Relaunch with this installed PTY provider instead of the current provider.
        #[arg(long, value_name = "PROVIDER")]
        provider: Option<String>,
        /// Include bounded, redacted history in a provider-changing fresh launch.
        #[arg(long, requires = "provider")]
        handoff_history: bool,
        /// Start a new session in the same tab. Live agents require --force; an existing exact restored session is never replaced.
        #[arg(long, conflicts_with_all = ["provider", "handoff_history"])]
        fresh: bool,
        /// Skip graceful exit and force-kill; also permits --fresh on a live agent.
        #[arg(long)]
        force: bool,
        /// Wait this long for graceful exit before force-killing (seconds)
        #[arg(long, default_value = "10")]
        grace_secs: u64,
    },
    /// Switch one idle agent Tab to another model. Only that Tab changes: it is relaunched on its own session and the command returns once the provider reports the new model. Providers without a live switch refuse; respawn them with `agent spawn PROVIDER --model X`.
    Model {
        /// Explicitly override the background-work restart guard and report the affected work.
        #[arg(long)]
        force: bool,
        /// Model id. Run `agent models PROVIDER` to list.
        /// Model id. Omit when changing only the service tier.
        name: Option<String>,
        /// Service tier id or `default` to clear the selection.
        #[arg(long, value_name = "TIER", conflicts_with = "list_speeds")]
        speed: Option<String>,
        /// Reasoning effort declared for the selected model.
        #[arg(long, value_name = "LEVEL", conflicts_with = "list_speeds")]
        reasoning_effort: Option<String>,
        /// Structured runtime mode declared by the selected provider.
        #[arg(long, value_name = "MODE", conflicts_with = "list_speeds")]
        runtime_mode: Option<String>,
        /// List this tab's currently available service tiers.
        #[arg(long, conflicts_with_all = ["speed", "reasoning_effort", "runtime_mode", "name", "force"])]
        list_speeds: bool,
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Continue this tab on another provider through a bounded handoff produced by the target.
    SwitchProvider {
        provider: String,
        #[arg(long)]
        tab: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        reasoning_effort: Option<String>,
        /// Stop the current turn before switching. Otherwise a busy tab refuses.
        #[arg(long)]
        force: bool,
    },
    /// Read a daemon-owned provider switch operation, including self-switch results.
    SwitchProviderStatus {
        operation: String,
        #[arg(long)]
        tab: Option<String>,
    },
    /// List available agent providers (claude, codex, ...) with their
    /// display names, default models, and capability flags.
    Providers,
    /// Open a new tab from a provider-declared session fork; bind only from its own session evidence.
    Fork {
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Inspect or manage a provider's CLI using its manifest commands.
    Cli {
        #[command(subcommand)]
        action: ProviderCliAction,
    },
    /// List available models for a specific provider.
    Models {
        /// Provider id (e.g. "claude"). Required.
        provider: String,
    },
    /// Spawn a new agent; roleless unless --role is provided.
    ///
    /// Two spawn shapes:
    ///   Worker (leaf task you own; reports back; never spawns further):
    ///     spawn claude --role Worker --parent-tab $CODETERM_TAB_ID --task "...
    ///     when done: domios agent report '<summary>'"
    ///   Orchestrator (independent objective / separate workspace / remote):
    ///     spawn codex --role Orchestrator --workspace WS_ID --task "..."
    ///
    /// The child lands in the parent's workspace, else in your own — never in
    /// the workspace the user has selected in the UI. Pass --workspace only to
    /// place it elsewhere on purpose.
    ///
    /// --task is launch-time input, NOT a routed inbound message: no
    /// return-address marker is injected. If the child must report or reply
    /// somewhere, put the exact report/send command in the task body.
    #[command(verbatim_doc_comment)]
    Spawn {
        /// Optional agent provider id. Without one, a uniquely advertised model id selects its provider; otherwise the configured default is used.
        agent: Option<String>,
        /// Tab title
        #[arg(long, num_args = 1)]
        title: Option<Vec<String>>,
        /// Short tab description (max 200 chars)
        #[arg(long, num_args = 1)]
        description: Option<Vec<String>>,
        /// Working directory: an existing directory, refused otherwise. A relative
        /// path resolves against your current directory. With --mesh it must be
        /// absolute on the peer; from Git Bash set MSYS_NO_PATHCONV=1 for POSIX paths.
        #[arg(long)]
        cwd: Option<String>,
        /// Role: Worker, Manager, Orchestrator, or Planner/Watcher profile sugar.
        #[arg(long)]
        role: Option<SpawnRole>,
        /// Worker prompt kind/profile, with or without a task: qa, release_manager or reviewer (read-only by default).
        #[arg(long, alias = "role-profile")]
        kind: Option<String>,
        /// Parent tab ID. Required for Workers; for a Planner, resolved
        /// from --plan (its Orchestrator) when omitted.
        #[arg(long = "parent-tab")]
        parent: Option<String>,
        /// Plan id to bind — required for `--role Planner` to auto-bind
        /// Plan.planner and deliver the peer prompt naming the Orchestrator.
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
        /// Where this worker's `agent report` should land. A tab id or
        /// `main`/`general` alias. With `--mesh PEER` it resolves on that peer,
        /// so a remote worker reports back to the spawning orchestrator. Defaults
        /// to the caller's own tab.
        #[arg(long)]
        report_to: Option<String>,
        /// Override whether an explicit Orchestrator may spawn children
        #[arg(long)]
        can_spawn: bool,
        /// Task to send after agent starts
        #[arg(long, allow_hyphen_values = true)]
        task: Option<String>,
        /// Read the task body from a file (or `-` for stdin) instead of --task.
        /// Local relative paths resolve from --cwd; mesh paths resolve on the
        /// caller. Content remains byte-exact. Mutually exclusive with --task.
        #[arg(long, value_name = "PATH")]
        task_file: Option<String>,
        /// Extra provider args. Put this after all Domios spawn options.
        #[arg(long, num_args = 1.., allow_hyphen_values = true)]
        args: Option<Vec<String>>,
        /// Target workspace ID. Defaults to the parent tab's workspace, then your
        /// own — never the workspace selected in the UI. Pass this only to place
        /// the agent in a different workspace on purpose.
        #[arg(long)]
        workspace: Option<String>,
        /// Model selector (e.g. "github-copilot/claude-haiku-4.5", "gpt-5"). The
        /// provider knows whether to emit `-m X` or `--model X` — caller passes
        /// the model string opaque. Run `domios agent models <provider>` to
        /// see what's available.
        #[arg(long)]
        model: Option<String>,
        #[arg(
            long,
            value_name = "TIER",
            help = "Provider-declared service tier for this launch"
        )]
        speed: Option<String>,
        /// Reasoning-effort level supported by the selected model. The provider's
        /// list is used when that model publishes no per-model levels; unsupported
        /// values are rejected, never silently downgraded.
        #[arg(long = "reasoning-effort")]
        reasoning_effort: Option<String>,
        /// Override the shell system prompt of a chatBackend shell; refused for PTY agents.
        #[arg(long = "system", allow_hyphen_values = true)]
        system_prompt: Option<String>,
        /// Select a provider-owned preset supported by the target provider.
        #[arg(long)]
        preset: Option<String>,
        /// Execution location declared by the provider. Remote tasks are one-shot tabs.
        #[arg(long, value_enum)]
        location: Option<SpawnLocation>,
        /// Remote environment ID or unambiguous label; never inferred.
        #[arg(
            long,
            requires = "location",
            required_if_eq("location", "cloud"),
            conflicts_with = "preset"
        )]
        environment: Option<String>,
        /// Watcher charter text.
        #[arg(long, allow_hyphen_values = true, conflicts_with = "charter_file")]
        charter: Option<String>,
        /// Read watcher charter text from a file.
        #[arg(long = "charter-file", value_name = "PATH", conflicts_with = "charter")]
        charter_file: Option<String>,
        /// Watcher tick interval, e.g. 90s, 5m, 1h.
        #[arg(long)]
        interval: Option<String>,
        /// Watcher nudge cooldown, e.g. 10m.
        #[arg(long = "nudge-cooldown")]
        nudge_cooldown: Option<String>,
        /// Max watcher actions per tick.
        #[arg(long = "max-actions")]
        max_actions: Option<u32>,
        /// Bound task id.
        #[arg(long = "task-id", env = "CODETERM_TASK_ID")]
        bound_task: Option<String>,
        /// Plan task to bind when spawning `--role Worker` (handle, id, or prefix).
        #[arg(long = "plan-task")]
        plan_task: Option<String>,
        /// Create and dispatch a visible exceptional task for this Worker.
        #[arg(long)]
        unplanned: bool,
        /// Why this Worker needs an unplanned task.
        #[arg(long, allow_hyphen_values = true)]
        reason: Option<String>,
        /// Attach this roleless agent to an existing Team, mirroring `tab new
        /// --team`. The calling tab (CODETERM_TAB_ID) must already belong to
        /// that Team. Rejected together with --plan-task, and with every --role
        /// but Orchestrator: a plan-bound agent takes its Team from that
        /// binding. `--team <id> --role Orchestrator` is the recovery add for a
        /// Team whose Orchestrator is gone and carries no other spawn flag.
        #[arg(long, value_name = "TEAM_ID", conflicts_with = "plan_task")]
        team: Option<String>,
        /// Declared execution intent. Plain mutating Workers require a task binding;
        /// Worker kinds may mutate task-less and default to `mutating`.
        #[arg(long = "execution-intent", value_parser = ["read_only", "mutating"])]
        execution_intent: Option<String>,
        /// Explicit OS confinement for this launch; Workers default to full access.
        #[arg(long, value_parser = ["read-only", "workspace-write"])]
        sandbox: Option<String>,
        /// Gate token this Worker may claim as executed; repeatable. Carried
        /// into its brief. For a task-bound Worker declare gates on the
        /// dispatch instead (`plan task dispatch --authorized-gate`).
        #[arg(
            long = "authorized-gate",
            value_name = "GATE",
            allow_hyphen_values = true
        )]
        authorized_gates: Vec<String>,
        #[arg(long = "provider-reserve-pct", default_value_t = 0.0)]
        provider_reserve_pct: f32,
        /// Declare the tab non-user-facing (bench/probe): its prompts and exits
        /// never notify the user; they are reported to its parent instead.
        #[arg(long = "non-user-facing")]
        non_user_facing: bool,
    },
}

#[cfg(test)]
#[path = "agent/tests.rs"]
mod tests;
#[derive(clap::Subcommand, Debug)]
pub enum ProviderCliAction {
    /// Show installed state, resolved binary, and commands for this OS.
    Status { provider: String },
    /// Run the manifest installer in a visible utility tab and report its result.
    Install { provider: String },
    /// Run the manifest updater in a visible utility tab and report its result.
    Update { provider: String },
}

#[cfg(test)]
#[path = "agent/sandbox_tests.rs"]
mod sandbox_tests;
