use clap::Subcommand;

#[derive(Subcommand)]
pub enum DelayedAction {
    /// List delayed messages for a tab
    List {
        /// Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },

    /// Queue a delayed message
    Create {
        /// Text to queue (positional, or omit and use --from-file/--stdin)
        text: Option<String>,

        /// Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,

        /// Relative delay: 45s, 10m, 1h30m, 2d. Bare number means minutes.
        #[arg(long, conflicts_with = "at")]
        delay: Option<String>,

        /// Absolute fire time: unix milliseconds, RFC3339, or local YYYY-MM-DDTHH:MM
        #[arg(long, conflicts_with = "delay")]
        at: Option<String>,

        /// Repeat delivery at an anchored interval (minimum 1m)
        #[arg(long)]
        every: Option<String>,

        /// Stop recurring delivery at an absolute time
        #[arg(long, conflicts_with = "count")]
        until: Option<String>,

        /// Stop after this many delivered or skipped ticks
        #[arg(long, conflicts_with = "until")]
        count: Option<u32>,

        /// Read text from a file instead of the positional arg
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,
        /// Read text from a relative file in the current agent tab's pad
        #[arg(long, value_name = "PATH", conflicts_with_all = ["text", "from_file", "stdin"])]
        from_pad: Option<String>,

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

        /// Replace the recurrence interval (minimum 1m)
        #[arg(long)]
        every: Option<String>,

        /// Replace the recurrence end time
        #[arg(long, conflicts_with = "count")]
        until: Option<String>,

        /// Replace the recurrence tick limit
        #[arg(long, conflicts_with = "until")]
        count: Option<u32>,

        /// Read replacement text from a file
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,
        /// Read replacement text from the current agent tab's pad
        #[arg(long, value_name = "PATH", conflicts_with_all = ["text", "from_file", "stdin"])]
        from_pad: Option<String>,

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
