use clap::{Subcommand, ValueEnum};

/// Per-request diagnostic override of the connected Supporter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SupporterMode {
    On,
    Off,
}

impl SupporterMode {
    pub fn enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

#[derive(Subcommand)]
pub enum SupporterAction {
    /// Show connection, activity, token state and budget.
    Status,
    /// List registered adapters and their discovered models.
    Models,
    /// Connect or activate an adapter, remembering its own model and token.
    Use {
        adapter: String,
        #[arg(long)]
        model: Option<String>,
    },
    /// Remove a saved connection and its token; removing the active one stops the Supporter.
    Disconnect { adapter: String },
    /// Manage the selected adapter's API token. The value is read from stdin
    /// only and is never printed.
    Token {
        #[command(subcommand)]
        action: SupporterTokenAction,
    },
}

#[derive(Subcommand)]
pub enum SupporterTokenAction {
    /// Store the token piped on stdin, e.g.
    /// `pbpaste | codeterm supporter token set --adapter typesafe-jev`.
    /// There is deliberately no argv flag for the value.
    Set {
        #[arg(long)]
        adapter: Option<String>,
    },
    /// Remove the stored token.
    Clear {
        #[arg(long)]
        adapter: Option<String>,
    },
    /// Report token presence, file path and mode (never the value). Without
    /// --adapter, every registered adapter is listed.
    Status {
        #[arg(long)]
        adapter: Option<String>,
    },
}
