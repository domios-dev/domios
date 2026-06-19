use clap::Subcommand;

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
/// is sent over the wire as-is. clap rejects anything outside the three.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum ReportStatusArg {
    Done,
    Blocked,
    Partial,
}

impl ReportStatusArg {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReportStatusArg::Done => "done",
            ReportStatusArg::Blocked => "blocked",
            ReportStatusArg::Partial => "partial",
        }
    }
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
    Report {
        /// Report message (also accepts --message alias)
        message: String,
        /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long)]
        pane: Option<String>,
        /// Task outcome: done | blocked | partial
        #[arg(long)]
        status: Option<ReportStatusArg>,
        /// Git branch the work landed on
        #[arg(long)]
        branch: Option<String>,
        /// Commit SHA produced
        #[arg(long)]
        sha: Option<String>,
        /// Changed files (comma-separated or repeat the flag)
        #[arg(long, value_delimiter = ',')]
        changed_files: Option<Vec<String>>,
        /// Test command that was run
        #[arg(long)]
        test_cmd: Option<String>,
        /// Result of the test command (e.g. "42 passed")
        #[arg(long)]
        test_result: Option<String>,
        /// Anything deliberately skipped
        #[arg(long)]
        skipped: Option<String>,
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
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Role: Orchestrator, Worker
        #[arg(long)]
        role: Option<SpawnRole>,
        /// Parent pane ID (for Workers)
        #[arg(long)]
        parent: Option<String>,
        /// Allow spawning children
        #[arg(long)]
        can_spawn: bool,
        /// Task to send after agent starts
        #[arg(long)]
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
        #[arg(long = "system")]
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

    #[test]
    fn report_status_accepts_done_blocked_partial() {
        for (input, expected) in [
            ("done", "done"),
            ("blocked", "blocked"),
            ("partial", "partial"),
        ] {
            let action = parse_report(&["msg", "--status", input]).expect("status should parse");
            match action {
                AgentAction::Report { status, .. } => {
                    assert_eq!(status.expect("status set").as_str(), expected);
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
            AgentAction::Report { changed_files, .. } => {
                assert_eq!(
                    changed_files,
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
            AgentAction::Report {
                status,
                branch,
                sha,
                changed_files,
                test_cmd,
                test_result,
                skipped,
                ..
            } => {
                assert!(status.is_none());
                assert!(branch.is_none());
                assert!(sha.is_none());
                assert!(changed_files.is_none());
                assert!(test_cmd.is_none());
                assert!(test_result.is_none());
                assert!(skipped.is_none());
            }
            _ => panic!("expected report action"),
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
