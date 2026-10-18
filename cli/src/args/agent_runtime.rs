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
    /// Recorded child thread id.
    #[arg(long, value_name = "THREAD", required = true)]
    pub thread: String,
    /// Current parent thread id from its runtime binding.
    #[arg(long, required = true)]
    pub root_thread_id: String,
    /// Current parent binding nonce.
    #[arg(long, required = true)]
    pub binding_nonce: String,
    /// Current parent runtime epoch.
    #[arg(long, required = true)]
    pub epoch: u64,
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
                action: AgentAction::ChildHistory(ChildHistoryArgs { epoch: 7, .. })
            }
        ));
    }
}
