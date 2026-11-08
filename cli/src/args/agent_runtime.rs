use clap::{Args, Subcommand};

#[derive(Subcommand, Debug)]
pub enum AgentUsageAction {
    /// Read this exact tab's provider usage snapshot.
    Read {
        #[arg(long, value_name = "TAB", required = true)]
        tab: String,
    },
    /// Consume one available reset credit on this exact tab's account.
    Reset {
        #[arg(long, value_name = "TAB", required = true)]
        tab: String,
        /// Confirm spending one reset credit.
        #[arg(long, required = true)]
        confirm: bool,
    },
}

#[derive(Args, Debug)]
pub struct ChildHistoryArgs {
    /// Parent agent tab id.
    #[arg(long, value_name = "TAB", required = true)]
    pub tab: String,
    /// List the parent's closed child tabs instead of reading a recorded thread.
    #[arg(
        long,
        conflicts_with_all = ["thread", "root_thread_id", "binding_nonce", "epoch", "cursor"]
    )]
    pub closed: bool,
    /// With --closed: read this closed child tab's transcript.
    #[arg(long, value_name = "TAB", requires = "closed")]
    pub child: Option<String>,
    /// With --child: only return messages from this index onward.
    #[arg(long, default_value_t = 0, requires = "child")]
    pub since: usize,
    /// Recorded child thread id.
    #[arg(long, value_name = "THREAD", required_unless_present = "closed")]
    pub thread: Option<String>,
    /// Current parent thread id from its runtime binding.
    #[arg(long, required_unless_present = "closed")]
    pub root_thread_id: Option<String>,
    /// Current parent binding nonce.
    #[arg(long, required_unless_present = "closed")]
    pub binding_nonce: Option<String>,
    /// Current parent runtime epoch.
    #[arg(long, required_unless_present = "closed")]
    pub epoch: Option<u64>,
    /// Opaque continuation cursor from the prior response.
    #[arg(long)]
    pub cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{AgentUsageAction, ChildHistoryArgs};
    use crate::{AgentAction, Cli, Commands};
    use clap::Parser;

    #[test]
    fn usage_and_child_history_require_exact_authority_targets() {
        let read =
            Cli::try_parse_from(["codeterm", "agent", "usage", "read", "--tab", "tab-a"]).unwrap();
        assert!(matches!(read.command, Commands::Agent {
            action: AgentAction::Usage { action: AgentUsageAction::Read { tab } }
        } if tab == "tab-a"));

        assert!(
            Cli::try_parse_from(["codeterm", "agent", "usage", "reset", "--tab", "tab-a"]).is_err()
        );
        let reset = Cli::try_parse_from([
            "codeterm",
            "agent",
            "usage",
            "reset",
            "--tab",
            "tab-a",
            "--confirm",
        ])
        .unwrap();
        assert!(matches!(reset.command, Commands::Agent {
            action: AgentAction::Usage { action: AgentUsageAction::Reset { tab, confirm: true } }
        } if tab == "tab-a"));

        assert!(Cli::try_parse_from([
            "codeterm",
            "agent",
            "child-history",
            "--tab",
            "parent",
            "--thread",
            "child"
        ])
        .is_err());
        let history = Cli::try_parse_from([
            "codeterm",
            "agent",
            "child-history",
            "--tab",
            "parent",
            "--thread",
            "child",
            "--root-thread-id",
            "root",
            "--binding-nonce",
            "nonce",
            "--epoch",
            "7",
        ])
        .unwrap();
        assert!(matches!(
            history.command,
            Commands::Agent {
                action: AgentAction::ChildHistory(ChildHistoryArgs {
                    epoch: Some(7),
                    closed: false,
                    ..
                })
            }
        ));
    }

    #[test]
    fn closed_child_history_needs_no_runtime_binding_and_refuses_to_mix_with_one() {
        let list = Cli::try_parse_from([
            "codeterm",
            "agent",
            "child-history",
            "--tab",
            "p",
            "--closed",
        ])
        .unwrap();
        assert!(matches!(
            list.command,
            Commands::Agent {
                action: AgentAction::ChildHistory(ChildHistoryArgs {
                    closed: true,
                    child: None,
                    ..
                })
            }
        ));
        let read = Cli::try_parse_from([
            "codeterm",
            "agent",
            "child-history",
            "--tab",
            "p",
            "--closed",
            "--child",
            "c",
            "--since",
            "3",
        ])
        .unwrap();
        assert!(matches!(
            read.command,
            Commands::Agent {
                action: AgentAction::ChildHistory(ChildHistoryArgs { since: 3, .. })
            }
        ));
        for refused in [
            vec![
                "codeterm",
                "agent",
                "child-history",
                "--tab",
                "p",
                "--child",
                "c",
            ],
            vec![
                "codeterm",
                "agent",
                "child-history",
                "--tab",
                "p",
                "--closed",
                "--thread",
                "t",
            ],
            vec![
                "codeterm",
                "agent",
                "child-history",
                "--tab",
                "p",
                "--closed",
                "--since",
                "1",
            ],
        ] {
            assert!(Cli::try_parse_from(refused).is_err());
        }
    }
}
