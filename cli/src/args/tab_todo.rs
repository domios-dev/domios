use clap::Subcommand;

#[derive(Subcommand)]
pub enum TabTodoAction {
    /// Seed a child tab's locked checklist from acceptance criteria (Orchestrator only).
    Init {
        /// Child tab id to initialize.
        #[arg(long = "tab", value_name = "ID", required = true)]
        tab: String,
        /// JSON array of acceptance-criteria strings from this tab's pad.
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "from_file",
            required_unless_present = "from_file"
        )]
        from_pad: Option<String>,
        /// JSON array of acceptance-criteria strings from a file.
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "from_pad",
            required_unless_present = "from_pad"
        )]
        from_file: Option<String>,
    },
    /// List the current tab's checklist or inspect one child as its parent.
    List {
        /// Child tab id. Requires this tab to be its authorized parent.
        #[arg(long = "tab", value_name = "ID")]
        tab: Option<String>,
    },
    /// Mark one checklist item complete.
    Complete {
        /// Stable checklist item id.
        item_id: String,
        /// Target tab id (default: this tab).
        #[arg(long = "tab", value_name = "ID")]
        tab: Option<String>,
    },
    /// Append ordered checks to a child checklist (QA Worker only).
    Append {
        /// Child tab id.
        #[arg(long = "tab", value_name = "ID", required = true)]
        tab: String,
        /// Current checklist generation, supplied by the authorized Orchestrator.
        #[arg(long, required = true)]
        generation: u64,
        /// JSON array of checklist drafts from this tab's pad.
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "from_file",
            required_unless_present = "from_file"
        )]
        from_pad: Option<String>,
        /// JSON array of checklist drafts from a file.
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "from_pad",
            required_unless_present = "from_pad"
        )]
        from_file: Option<String>,
    },
    /// Amend one checklist item (Orchestrator only).
    Amend {
        /// Stable checklist item id.
        item_id: String,
        /// Child tab id.
        #[arg(long = "tab", value_name = "ID", required = true)]
        tab: String,
        /// One JSON checklist draft from this tab's pad.
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "from_file",
            required_unless_present = "from_file"
        )]
        from_pad: Option<String>,
        /// One JSON checklist draft from a file.
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "from_pad",
            required_unless_present = "from_pad"
        )]
        from_file: Option<String>,
    },
    /// Reset/rewrite a checklist as the user, starting a new generation.
    Reset {
        /// Target tab id (default: this tab when invoked from the host).
        #[arg(long = "tab", value_name = "ID")]
        tab: Option<String>,
        /// JSON array of checklist drafts from a file.
        #[arg(long, value_name = "PATH")]
        from_file: String,
    },
    /// Clear one item or the entire checklist as the user.
    Clear {
        /// Stable checklist item id; omit when using --all.
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        item_id: Option<String>,
        /// Clear every item in the current generation.
        #[arg(long, conflicts_with = "item_id")]
        all: bool,
        /// Target tab id (default: this tab when invoked from the host).
        #[arg(long = "tab", value_name = "ID")]
        tab: Option<String>,
    },
}
