use clap::{Subcommand, ValueEnum};

/// Per-request override of `[supporter.memory_rerank].enabled`.
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
    /// Enable both Supporter Agent features.
    Enable,
    /// Disable both Supporter Agent features.
    Disable,
    /// Show the selected adapter, model, token state and feature state.
    Status,
    /// List registered adapters and their discovered models.
    Models,
    /// Select a registered adapter and optionally one of its listed models.
    Use {
        adapter: String,
        #[arg(long)]
        model: Option<String>,
    },
    /// Read or change the feature switches.
    Features {
        #[arg(long, value_enum, value_name = "on|off")]
        memory_rerank: Option<SupporterMode>,
        #[arg(long, value_enum, value_name = "on|off")]
        tab_state: Option<SupporterMode>,
    },
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
    Set { #[arg(long)] adapter: Option<String> },
    /// Remove the stored token.
    Clear { #[arg(long)] adapter: Option<String> },
    /// Report token presence, file path and mode (never the value). Without
    /// --adapter, every registered adapter is listed.
    Status {
        #[arg(long)]
        adapter: Option<String>,
    },
}
