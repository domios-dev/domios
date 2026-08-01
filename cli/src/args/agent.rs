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
    /// Tab ID (default: $CODETERM_TAB_ID or focused tab)
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

#[derive(Subcommand)]
pub enum AgentAction {
    /// Mark a tab as orchestrator (top-level, can spawn workers)
    SetOrchestrator {
        /// Tab ID (default: current focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },

    /// Link a tab as worker under a parent
    Link {
        /// Parent tab ID (orchestrator)
        #[arg(long = "parent-tab")]
        parent: String,
        /// Tab ID to link (default: current focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Remove a tab's role (also orphans children)
    Unlink {
        /// Tab ID (default: current focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Toggle whether a tab can spawn children
    AllowSpawn {
        /// Tab ID (default: current focused tab)
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
        /// Tab ID (default: $CODETERM_TAB_ID or focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Plan id (default: $CODETERM_PLAN_ID)
        #[arg(long, env = "CODETERM_PLAN_ID")]
        plan: Option<String>,
        /// Task id (default: $CODETERM_TASK_ID)
        #[arg(long, env = "CODETERM_TASK_ID")]
        task: Option<String>,
    },
    /// List an orchestrator's workers from the durable ledger (survives daemon
    /// restart): branch / model / status / last-seen, one row per worker.
    Ledger {
        /// Orchestrator tab ID (default: current focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Compact agent context (sends /compact to Claude)
    Compact {
        /// Tab ID (default: $CODETERM_TAB_ID or focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Restart agent (graceful exit; resumes only when the tab has a valid provider session)
    Restart {
        /// Tab ID (default: $CODETERM_TAB_ID or focused tab)
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
        /// Tab ID (default: $CODETERM_TAB_ID or focused tab)
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
    /// --task is launch-time input, NOT a routed inbound message: no
    /// return-address marker is injected. If the child must report or reply
    /// somewhere, put the exact report/send command in the task body.
    #[command(verbatim_doc_comment)]
    Spawn {
        /// Agent provider id. Run `codeterm agent providers` to list registered providers (e.g. claude, codex, opencode). Default: claude.
        #[arg(default_value = "claude")]
        agent: String,
        /// Tab title
        #[arg(long, num_args = 1..)]
        title: Option<Vec<String>>,
        /// Short tab description (max 200 chars)
        #[arg(long, num_args = 1..)]
        description: Option<Vec<String>>,
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Role: Worker, Manager, Orchestrator, or Planner/Watcher profile sugar.
        #[arg(long)]
        role: Option<SpawnRole>,
        /// Parent tab ID (for Workers)
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
        /// Bypasses the caller's shell, so backticks / $(...) in the body are
        /// never evaluated. Mutually exclusive with --task.
        #[arg(long, value_name = "PATH")]
        task_file: Option<String>,
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
        /// Reasoning-effort level (provider-specific, e.g. codex low/medium/high).
        /// Capability-validated: an unsupported value is rejected, never silently
        /// downgraded. Run `codeterm agent models <provider>` to see supported levels.
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

    fn parse_top_report(args: &[&str]) -> Result<ReportArgs, clap::Error> {
        let mut full = vec!["codeterm", "report"];
        full.extend_from_slice(args);
        let cli = Cli::try_parse_from(full)?;
        match cli.command {
            crate::Commands::Report(args) => Ok(args),
            _ => panic!("expected report command"),
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
    fn top_level_report_uses_same_parser() {
        let args = parse_top_report(&[
            "--status",
            "in_review",
            "--branch",
            "lane/report-cli-planning",
            "--base",
            "abc123",
            "ready",
        ])
        .expect("top-level report parses");

        assert_eq!(args.message.as_deref(), Some("ready"));
        assert_eq!(args.status.expect("status set").as_str(), "in_review");
        assert_eq!(args.branch.as_deref(), Some("lane/report-cli-planning"));
        assert_eq!(args.base.as_deref(), Some("abc123"));
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
    fn spawn_title_accepts_unquoted_words_until_next_flag() {
        let cli = Cli::try_parse_from([
            "codeterm",
            "agent",
            "spawn",
            "claude",
            "--title",
            "perf",
            "brainstorm",
            "opus",
            "--task",
            "review startup",
        ])
        .expect("unquoted title words should parse");
        match cli.command {
            crate::Commands::Agent {
                action: AgentAction::Spawn { title, task, .. },
            } => {
                assert_eq!(
                    title.map(|words| words.join(" ")).as_deref(),
                    Some("perf brainstorm opus")
                );
                assert_eq!(task.as_deref(), Some("review startup"));
            }
            _ => panic!("expected agent spawn"),
        }
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
