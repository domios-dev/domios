use clap::{Args, Subcommand};

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum SpawnRole {
    #[value(name = "Orchestrator")]
    Orchestrator,
    #[value(name = "Worker")]
    Worker,
}

impl SpawnRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpawnRole::Orchestrator => "Orchestrator",
            SpawnRole::Worker => "Worker",
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
    /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
    #[arg(long)]
    pub pane: Option<String>,
    /// Plan id to append this report to (default: $CODETERM_PLAN_ID)
    #[arg(long, env = "CODETERM_PLAN_ID")]
    pub plan: Option<String>,
    /// Task id to append this report to (default: $CODETERM_TASK_ID)
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
}

#[derive(Subcommand)]
pub enum AgentAction {
    /// Mark a pane as orchestrator (top-level, can spawn workers)
    SetOrchestrator {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
    },

    /// Link a pane as worker under a parent
    Link {
        /// Parent pane ID (orchestrator)
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
    Report(ReportArgs),
    /// Send an immediate message to a running agent pane
    Message {
        /// Message text (positional, or omit and use --from-file/--stdin)
        #[arg(allow_hyphen_values = true)]
        text: Option<String>,
        /// Target agent pane ID
        #[arg(long, alias = "id")]
        pane: String,
        /// Read message text from a file instead of the positional arg
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,
        /// Read message text from stdin
        #[arg(long, visible_alias = "from-stdin", conflicts_with = "text")]
        stdin: bool,
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
    /// List an orchestrator's workers from the durable ledger (survives daemon
    /// restart): branch / model / status / last-seen, one row per worker.
    Ledger {
        /// Orchestrator pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Compact agent context (sends /compact to Claude)
    Compact {
        /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Restart agent (graceful exit; resumes only when the pane has a valid session)
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
        /// Short pane description (max 200 chars)
        #[arg(long)]
        description: Option<String>,
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Role: Orchestrator, Worker
        #[arg(long)]
        role: Option<SpawnRole>,
        /// Parent pane ID (for Workers)
        #[arg(long)]
        parent: Option<String>,
        /// Where this worker's `agent report` should land. A pane id or
        /// `main`/`general` alias. With `--mesh PEER` it resolves on that peer,
        /// so a remote worker reports back to the spawning orchestrator. Defaults
        /// to the caller's own pane.
        #[arg(long)]
        report_to: Option<String>,
        /// Allow spawning children
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
        /// Override the shell system prompt when spawning a chatBackend shell.
        #[arg(long = "system", allow_hyphen_values = true)]
        system_prompt: Option<String>,
        /// Select a chatBackend shell preset. Omit to use the plugin default preset.
        #[arg(long)]
        preset: Option<String>,
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

    fn parse_message(args: &[&str]) -> Result<AgentAction, clap::Error> {
        let mut full = vec!["codeterm", "agent", "message"];
        full.extend_from_slice(args);
        let cli = Cli::try_parse_from(full)?;
        match cli.command {
            crate::Commands::Agent { action } => Ok(action),
            _ => panic!("expected agent subcommand"),
        }
    }

    #[test]
    fn message_requires_target_pane() {
        match parse_message(&["hello"]) {
            Err(e) => assert_eq!(e.kind(), clap::error::ErrorKind::MissingRequiredArgument),
            Ok(_) => panic!("message without --pane must be rejected"),
        }
    }

    #[test]
    fn message_parses_text_and_target_pane() {
        let action = parse_message(&["hello", "--pane", "abc123"]).expect("message parses");
        match action {
            AgentAction::Message {
                text,
                pane,
                from_file,
                stdin,
            } => {
                assert_eq!(text.as_deref(), Some("hello"));
                assert_eq!(pane, "abc123");
                assert!(from_file.is_none());
                assert!(!stdin);
            }
            _ => panic!("expected message action"),
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
    fn report_without_flags_leaves_structured_fields_unset() {
        let action = parse_report(&["just a message"]).unwrap();
        match action {
            AgentAction::Report(args) => {
                assert!(args.status.is_none());
                assert!(args.branch.is_none());
                assert!(args.sha.is_none());
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
            "ready",
        ])
        .expect("top-level report parses");

        assert_eq!(args.message.as_deref(), Some("ready"));
        assert_eq!(args.status.expect("status set").as_str(), "in_review");
        assert_eq!(args.branch.as_deref(), Some("lane/report-cli-planning"));
    }

    #[test]
    fn ledger_parses_pane_and_global_json() {
        let cli =
            Cli::try_parse_from(["codeterm", "--json", "agent", "ledger", "--pane", "orch-1"])
                .expect("ledger should parse");
        assert!(cli.json, "global --json applies to ledger output");
        match cli.command {
            crate::Commands::Agent {
                action: AgentAction::Ledger { pane },
            } => assert_eq!(pane.as_deref(), Some("orch-1")),
            _ => panic!("expected agent ledger action"),
        }
    }

    #[test]
    fn ledger_pane_is_optional() {
        let cli = Cli::try_parse_from(["codeterm", "agent", "ledger"]).expect("ledger w/o pane");
        match cli.command {
            crate::Commands::Agent {
                action: AgentAction::Ledger { pane },
            } => assert!(pane.is_none()),
            _ => panic!("expected agent ledger action"),
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
