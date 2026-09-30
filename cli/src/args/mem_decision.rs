use clap::{Subcommand, ValueEnum};

pub const DEFAULT_DECISION_ADAPTER: &str = "typesafe-jev";

/// Per-request override of `[memory.decision_rerank].enabled`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum DecisionRerankMode {
    On,
    Off,
}

impl DecisionRerankMode {
    pub fn enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

#[derive(Subcommand)]
pub enum MemDecisionAction {
    /// Manage a decision adapter's API token. The value is read from stdin
    /// only and is never printed.
    Token {
        #[command(subcommand)]
        action: MemDecisionTokenAction,
    },
    /// Show the decision re-rank config, registered adapters and token presence.
    Status,
}

#[derive(Subcommand)]
pub enum MemDecisionTokenAction {
    /// Store the token piped on stdin, e.g.
    /// `pbpaste | codeterm mem decision token set --adapter typesafe-jev`.
    /// There is deliberately no argv flag for the value.
    Set {
        #[arg(long, default_value = DEFAULT_DECISION_ADAPTER)]
        adapter: String,
    },
    /// Remove the stored token.
    Clear {
        #[arg(long, default_value = DEFAULT_DECISION_ADAPTER)]
        adapter: String,
    },
    /// Report token presence, file path and mode (never the value). Without
    /// --adapter, every registered adapter is listed.
    Status {
        #[arg(long)]
        adapter: Option<String>,
    },
}
