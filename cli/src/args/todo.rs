use clap::Subcommand;

#[derive(Subcommand)]
pub enum TodoAction {
    /// Read this agent tab's current bounded checklist.
    List,
    /// Share this tab's checklist with its parent; work may continue immediately.
    Submit,
    /// Mark a child's shared checklist reviewed.
    Approve {
        #[arg(long, value_name = "ID")]
        tab: String,
        #[arg(long, value_name = "PATH")]
        note_from_pad: Option<String>,
    },
    /// Review and amend a child's checklist; seeded criteria stay locked.
    /// Input: [{"id"?: string, "content"?: string, "status": "pending|in_progress|completed|blocked|skipped", "activeForm"?: string, "link"?: string, "reason"?: string}]. New items require content; activeForm defaults to content. Blocked/skipped require reason.
    Amend {
        #[arg(long, value_name = "ID")]
        tab: String,
        #[arg(long, value_name = "PATH")]
        from_pad: String,
        #[arg(long, value_name = "PATH")]
        note_from_pad: Option<String>,
    },
    /// Declare this tab's whole checklist: created on first write, then reconciled; locked items stay.
    Write {
        /// JSON array of {content,status,activeForm?,id?,link?,reason?}; activeForm defaults to content.
        #[arg(long, value_name = "PATH")]
        from_pad: String,
    },
}
