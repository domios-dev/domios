use clap::Subcommand;

#[derive(Subcommand)]
pub enum DelayedAction {
    /// List delayed messages for a tab
    List {
        /// Target tab ID (default: $CODETERM_TAB_ID or focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },

    /// Queue a delayed message
    Create {
        /// Text to queue (positional, or omit and use --from-file/--stdin)
        text: Option<String>,

        /// Target tab ID (default: $CODETERM_TAB_ID or focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,

        /// Relative delay: 45s, 10m, 1h30m, 2d. Bare number means minutes.
        #[arg(long, conflicts_with = "at")]
        delay: Option<String>,

        /// Absolute fire time: unix milliseconds, RFC3339, or local YYYY-MM-DDTHH:MM
        #[arg(long, conflicts_with = "delay")]
        at: Option<String>,

        /// Read text from a file instead of the positional arg
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,

        /// Read text from stdin
        #[arg(long, visible_alias = "from-stdin", conflicts_with = "text")]
        stdin: bool,
    },

    /// Edit a queued or failed delayed message
    Edit {
        /// Delayed message ID
        id: String,

        /// Replacement text
        #[arg(long)]
        text: Option<String>,

        /// Relative delay from now: 45s, 10m, 1h30m, 2d. Bare number means minutes.
        #[arg(long, conflicts_with = "at")]
        delay: Option<String>,

        /// Absolute fire time: unix milliseconds, RFC3339, or local YYYY-MM-DDTHH:MM
        #[arg(long, conflicts_with = "delay")]
        at: Option<String>,

        /// Read replacement text from a file
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,

        /// Read replacement text from stdin
        #[arg(long, visible_alias = "from-stdin", conflicts_with = "text")]
        stdin: bool,
    },

    /// Delete a delayed message
    Delete {
        /// Delayed message ID
        id: String,
    },

    /// Send a delayed message immediately
    SendNow {
        /// Delayed message ID
        id: String,
    },
}
