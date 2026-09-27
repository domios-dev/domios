use clap::{Args, Subcommand};

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
    /// Task outcome: in_review | blocked | partial | done
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
    /// Mark a tab as orchestrator (top-level, can spawn workers)
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
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
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
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// List available agent providers (claude, codex, ...) with their
    /// display names, default models, and capability flags.
    Providers,
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
    ///     when done: codeterm agent report '<summary>'"
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
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Role: Worker, Manager, Orchestrator, or Planner/Watcher profile sugar.
        #[arg(long)]
        role: Option<SpawnRole>,
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
        /// Relative paths resolve from --cwd. Content bypasses the caller's shell
        /// and remains byte-exact. Mutually exclusive with --task.
        #[arg(long, value_name = "PATH")]
        task_file: Option<String>,
        /// Extra provider args. Put this after all CodeTerm spawn options.
        #[arg(long, num_args = 1.., allow_hyphen_values = true)]
        args: Option<Vec<String>>,
        /// Target workspace ID. Defaults to the parent tab's workspace, then your
        /// own — never the workspace selected in the UI. Pass this only to place
        /// the agent in a different workspace on purpose.
        #[arg(long)]
        workspace: Option<String>,
        /// Model selector (e.g. "github-copilot/claude-haiku-4.5", "gpt-5"). The
        /// provider knows whether to emit `-m X` or `--model X` — caller passes
        /// the model string opaque. Run `codeterm agent models <provider>` to
        /// see what's available.
        #[arg(long)]
        model: Option<String>,
        /// Reasoning-effort level supported by the selected model. The provider's
        /// list is used when that model publishes no per-model levels; unsupported
        /// values are rejected, never silently downgraded.
        #[arg(long = "reasoning-effort")]
        reasoning_effort: Option<String>,
        /// Override the shell system prompt when spawning a chatBackend shell.
        #[arg(long = "system", allow_hyphen_values = true)]
        system_prompt: Option<String>,
        /// Select a chatBackend shell preset. Omit to use the plugin default preset.
        #[arg(long)]
        preset: Option<String>,
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
        /// Declared execution intent. `mutating` requires a task binding;
        /// `read_only` is also the task-less verification-Worker shape.
        #[arg(long = "execution-intent", value_parser = ["read_only", "mutating"])]
        execution_intent: Option<String>,
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
        /// Declare the tab non-user-facing (bench/probe): its interactive
        /// prompts never raise user-visible Input-needed notifications.
        #[arg(long = "non-user-facing")]
        non_user_facing: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use clap::{CommandFactory, Parser};

    fn parse_report(args: &[&str]) -> Result<AgentAction, clap::Error> {
        let mut full = vec!["codeterm", "agent", "report"];
        full.extend_from_slice(args);
        let cli = Cli::try_parse_from(full)?;
        match cli.command {
            crate::Commands::Agent { action } => Ok(action),
            _ => panic!("expected agent subcommand"),
        }
    }

    fn parse_spawn(args: &[&str]) -> Result<AgentAction, clap::Error> {
        let mut full = vec!["codeterm", "agent", "spawn"];
        full.extend_from_slice(args);
        let cli = Cli::try_parse_from(full)?;
        match cli.command {
            crate::Commands::Agent { action } => Ok(action),
            _ => panic!("expected agent subcommand"),
        }
    }

    #[test]
    fn report_status_accepts_known_values() {
        for (input, expected) in [
            ("done", "done"),
            ("in_review", "in_review"),
            ("blocked", "blocked"),
            ("partial", "partial"),
        ] {
            let action = parse_report(&["msg", "--status", input]).expect("status should parse");
            match action {
                AgentAction::Report(args) => {
                    assert_eq!(args.status.expect("status set").as_str(), expected);
                }
                _ => panic!("expected report action"),
            }
        }
    }

    #[test]
    fn report_status_rejects_garbage() {
        match parse_report(&["msg", "--status", "frobnicate"]) {
            Err(e) => assert_eq!(e.kind(), clap::error::ErrorKind::InvalidValue),
            Ok(_) => panic!("garbage --status must be rejected"),
        }
    }

    #[test]
    fn report_changed_files_splits_on_comma() {
        let action = parse_report(&["msg", "--changed-files", "a.rs,b.rs,c.rs"]).unwrap();
        match action {
            AgentAction::Report(args) => {
                assert_eq!(
                    args.changed_files,
                    Some(vec!["a.rs".into(), "b.rs".into(), "c.rs".into()])
                );
            }
            _ => panic!("expected report action"),
        }
    }

    #[test]
    fn explicit_report_plan_and_task_take_precedence() {
        let action =
            parse_report(&["msg", "--plan", "pl_explicit", "--task", "task_explicit"]).unwrap();
        match action {
            AgentAction::Report(args) => {
                assert_eq!(args.plan.as_deref(), Some("pl_explicit"));
                assert_eq!(args.task.as_deref(), Some("task_explicit"));
            }
            _ => panic!("expected agent report"),
        }
    }

    #[test]
    fn report_without_flags_leaves_structured_fields_unset() {
        let action = parse_report(&["just a message"]).unwrap();
        match action {
            AgentAction::Report(args) => {
                assert!(args.status.is_none());
                assert!(args.branch.is_none());
                assert!(args.sha.is_none());
                assert!(args.base.is_none());
                assert!(args.changed_files.is_none());
                assert!(args.test_cmd.is_none());
                assert!(args.test_result.is_none());
                assert!(args.skipped.is_none());
            }
            _ => panic!("expected report action"),
        }
    }

    #[test]
    fn agent_report_parses_full_structured_form() {
        let action = parse_report(&[
            "--status",
            "in_review",
            "--branch",
            "lane/report-cli-planning",
            "--base",
            "abc123",
            "ready",
        ])
        .expect("agent report parses");

        match action {
            AgentAction::Report(args) => {
                assert_eq!(args.message.as_deref(), Some("ready"));
                assert_eq!(args.status.expect("status set").as_str(), "in_review");
                assert_eq!(args.branch.as_deref(), Some("lane/report-cli-planning"));
                assert_eq!(args.base.as_deref(), Some("abc123"));
            }
            _ => panic!("expected agent report"),
        }
    }

    #[test]
    fn ledger_parses_tab_and_global_json() {
        let cli = Cli::try_parse_from(["codeterm", "--json", "agent", "ledger", "--tab", "orch-1"])
            .expect("ledger should parse");
        assert!(cli.json, "global --json applies to ledger output");
        match cli.command {
            crate::Commands::Agent {
                action: AgentAction::Ledger { tab },
            } => assert_eq!(tab.as_deref(), Some("orch-1")),
            _ => panic!("expected agent ledger action"),
        }
    }

    #[test]
    fn ack_parses_without_verbose_payload() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let cli = Cli::try_parse_from(["codeterm", "agent", "ack"]).expect("ack should parse");
        match cli.command {
            crate::Commands::Agent {
                action: AgentAction::Ack { tab, plan, task },
            } => {
                assert!(tab.is_none());
                assert!(plan.is_none());
                assert!(task.is_none());
            }
            _ => panic!("expected agent ack action"),
        }
    }

    #[test]
    fn startup_hold_parses_attempt_fence() {
        let hold = Cli::try_parse_from([
            "codeterm",
            "agent",
            "startup-hold",
            "--tab",
            "child",
            "--attempt",
            "launch-a",
        ])
        .expect("startup hold parses");
        assert!(matches!(
            hold.command,
            crate::Commands::Agent {
                action: AgentAction::StartupHold { ref tab, ref attempt }
            } if tab == "child" && attempt == "launch-a"
        ));
    }

    #[test]
    fn startup_ack_has_no_spoofable_identity_flags() {
        let ack =
            Cli::try_parse_from(["codeterm", "agent", "startup-ack"]).expect("startup ack parses");
        assert!(matches!(
            ack.command,
            crate::Commands::Agent {
                action: AgentAction::StartupAck
            }
        ));
    }

    #[test]
    fn ledger_tab_is_optional() {
        let cli = Cli::try_parse_from(["codeterm", "agent", "ledger"]).expect("ledger w/o tab");
        match cli.command {
            crate::Commands::Agent {
                action: AgentAction::Ledger { tab },
            } => assert!(tab.is_none()),
            _ => panic!("expected agent ledger action"),
        }
    }

    #[test]
    fn spawn_without_provider_keeps_model_routing_available() {
        let cli = Cli::try_parse_from(["codeterm", "agent", "spawn", "--model", "sonnet"])
            .expect("provider is optional when a model can identify it");
        match cli.command {
            crate::Commands::Agent {
                action: AgentAction::Spawn { agent, model, .. },
            } => {
                assert_eq!(agent, None);
                assert_eq!(model.as_deref(), Some("sonnet"));
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn spawn_planner_role_maps_to_worker_profile() {
        let cli =
            Cli::try_parse_from(["codeterm", "agent", "spawn", "claude", "--role", "Planner"])
                .expect("planner role parses");
        match cli.command {
            crate::Commands::Agent {
                action: AgentAction::Spawn { role, .. },
            } => {
                let role = role.expect("role set");
                assert_eq!(role.as_str(), "Manager");
                assert_eq!(role.role_profile(), Some("planner"));
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn spawn_watcher_role_maps_to_manager_profile() {
        let cli = Cli::try_parse_from([
            "codeterm", "agent", "spawn", "lmstudio", "--role", "Watcher",
        ])
        .expect("watcher role parses");
        match cli.command {
            crate::Commands::Agent {
                action: AgentAction::Spawn { role, .. },
            } => {
                let role = role.expect("role set");
                assert_eq!(role.as_str(), "Manager");
                assert_eq!(role.role_profile(), Some("watcher"));
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn spawn_worker_plan_task_parses() {
        let cli = Cli::try_parse_from([
            "codeterm",
            "agent",
            "spawn",
            "claude",
            "--role",
            "Worker",
            "--plan",
            "pl_test",
            "--plan-task",
            "my-task",
        ])
        .expect("spawn with plan-task parses");
        match cli.command {
            crate::Commands::Agent {
                action:
                    AgentAction::Spawn {
                        role,
                        plan,
                        plan_task,
                        ..
                    },
            } => {
                assert!(matches!(role, Some(SpawnRole::Worker)));
                assert_eq!(plan.as_deref(), Some("pl_test"));
                assert_eq!(plan_task.as_deref(), Some("my-task"));
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn agent_spawn_unplanned_parses_reason() {
        let action = parse_spawn(&[
            "codex",
            "--role",
            "Worker",
            "--unplanned",
            "--reason",
            "urgent probe",
        ])
        .expect("unplanned Worker spawn parses");
        match action {
            AgentAction::Spawn {
                role,
                unplanned,
                reason,
                ..
            } => {
                assert!(matches!(role, Some(SpawnRole::Worker)));
                assert!(unplanned);
                assert_eq!(reason.as_deref(), Some("urgent probe"));
            }
            _ => panic!("expected agent spawn"),
        }
    }

    /// The verification-Worker spawn: read-only, no `--plan-task`, and its
    /// declared gate tokens travel as repeats of the same flag the dispatch
    /// path already uses.
    #[test]
    fn spawn_read_only_worker_parses_without_a_plan_task() {
        let action = parse_spawn(&[
            "codex",
            "--role",
            "Worker",
            "--plan",
            "pl_test",
            "--execution-intent",
            "read_only",
            "--authorized-gate",
            "./build.sh test-one app verifier",
            "--authorized-gate",
            "./build.sh verify",
        ])
        .expect("a read-only Worker needs no plan task");
        match action {
            AgentAction::Spawn {
                role,
                plan,
                plan_task,
                execution_intent,
                authorized_gates,
                ..
            } => {
                assert!(matches!(role, Some(SpawnRole::Worker)));
                assert_eq!(plan.as_deref(), Some("pl_test"));
                assert!(plan_task.is_none());
                assert_eq!(execution_intent.as_deref(), Some("read_only"));
                assert_eq!(
                    authorized_gates,
                    vec![
                        "./build.sh test-one app verifier".to_string(),
                        "./build.sh verify".to_string()
                    ]
                );
            }
            _ => panic!("expected agent spawn"),
        }
    }

    /// The reported defect: clap demanded `--plan-task` for any
    /// `--execution-intent`, so the documented verifier could not be spawned.
    #[test]
    fn execution_intent_no_longer_requires_a_plan_task_at_parse_time() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let action = parse_spawn(&[
            "codex",
            "--role",
            "Worker",
            "--execution-intent",
            "read_only",
        ])
        .expect("--execution-intent must parse on its own");
        match action {
            AgentAction::Spawn {
                execution_intent,
                plan,
                plan_task,
                ..
            } => {
                assert_eq!(execution_intent.as_deref(), Some("read_only"));
                assert!(plan.is_none());
                assert!(plan_task.is_none());
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn spawn_provider_args_preserve_hyphen_values_after_codeterm_bindings() {
        let action = parse_spawn(&[
            "claude",
            "--role",
            "Worker",
            "--plan",
            "pl_x",
            "--task",
            "do x",
            "--args",
            "--resume",
            "1234-uuid",
            "--model",
            "gpt-5",
        ])
        .expect("provider args should parse");

        match action {
            AgentAction::Spawn {
                role,
                plan,
                task,
                args,
                model,
                ..
            } => {
                assert!(matches!(role, Some(SpawnRole::Worker)));
                assert_eq!(plan.as_deref(), Some("pl_x"));
                assert_eq!(task.as_deref(), Some("do x"));
                assert_eq!(
                    args,
                    Some(vec![
                        "--resume".into(),
                        "1234-uuid".into(),
                        "--model".into(),
                        "gpt-5".into(),
                    ])
                );
                assert!(model.is_none());
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn spawn_without_provider_args_is_unaffected() {
        let action = parse_spawn(&["claude"]).expect("spawn without options should parse");
        match action {
            AgentAction::Spawn { role, args, .. } => {
                assert!(role.is_none());
                assert!(args.is_none());
            }
            _ => panic!("expected agent spawn"),
        }

        let action = parse_spawn(&["claude", "--role", "Worker"]).expect("role should still parse");
        match action {
            AgentAction::Spawn { role, args, .. } => {
                assert!(matches!(role, Some(SpawnRole::Worker)));
                assert!(args.is_none());
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn spawn_title_does_not_consume_following_flags() {
        let cli = Cli::try_parse_from([
            "codeterm",
            "agent",
            "spawn",
            "claude",
            "--title",
            "Team routing smoke",
            "--reasoning-effort",
            "low",
            "--description",
            "Team containment probe",
            "--non-user-facing",
        ])
        .expect("spawn flags after title should parse independently");
        match cli.command {
            crate::Commands::Agent {
                action:
                    AgentAction::Spawn {
                        title,
                        description,
                        reasoning_effort,
                        non_user_facing,
                        ..
                    },
            } => {
                assert_eq!(
                    title.map(|words| words.join(" ")).as_deref(),
                    Some("Team routing smoke")
                );
                assert_eq!(reasoning_effort.as_deref(), Some("low"));
                assert_eq!(
                    description.map(|words| words.join(" ")).as_deref(),
                    Some("Team containment probe")
                );
                assert!(non_user_facing);
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn spawn_team_attaches_a_roleless_agent() {
        let action = parse_spawn(&["claude", "--team", "team_alpha"]).expect("--team parses");
        match action {
            AgentAction::Spawn {
                team,
                role,
                plan_task,
                ..
            } => {
                assert_eq!(team.as_deref(), Some("team_alpha"));
                assert!(role.is_none());
                assert!(plan_task.is_none());
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn spawn_team_conflicts_with_plan_task() {
        let error = parse_spawn(&["claude", "--team", "team_alpha", "--plan-task", "task_one"])
            .expect_err("--team must not combine with a plan binding");
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    /// codeterm#418: the role that anchors a Team is the one `--role` `--team`
    /// accepts, so a stranded Team can be handed a new Orchestrator. Every other
    /// role is refused later, by code, not by clap.
    #[test]
    fn spawn_team_parses_with_an_anchoring_role() {
        let parsed = parse_spawn(&["claude", "--team", "team_alpha", "--role", "Orchestrator"])
            .expect("--team --role Orchestrator parses");
        match parsed {
            AgentAction::Spawn { team, role, .. } => {
                assert_eq!(team.as_deref(), Some("team_alpha"));
                assert!(matches!(role, Some(SpawnRole::Orchestrator)));
            }
            _ => panic!("expected agent spawn"),
        }
        assert!(
            parse_spawn(&["claude", "--team", "team_alpha", "--role", "Worker"]).is_ok(),
            "a non-anchoring role parses and is refused by the spawn verb, not by clap"
        );
    }

    #[test]
    fn set_orchestrator_help_mentions_workers_not_leads() {
        let help = Cli::command()
            .find_subcommand_mut("agent")
            .unwrap()
            .find_subcommand_mut("set-orchestrator")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(help.contains("workers"), "{help}");
        assert!(!help.to_ascii_lowercase().contains("lead"), "{help}");
    }
}
