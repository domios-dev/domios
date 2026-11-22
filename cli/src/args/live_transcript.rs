use clap::{Args, Subcommand};

#[derive(Args)]
pub struct TranscriptOwner {
    /// Exact live transcript owner tab ID
    #[arg(long, env = "CODETERM_TAB_ID")]
    pub tab: String,
}

#[derive(Args)]
pub struct TranscriptAdvance {
    /// Active binding revision advertised by the owner or route
    #[arg(long)]
    pub revision: u64,
    /// Advance through this fully consumed sequence
    #[arg(long)]
    pub through_seq: u64,
}

#[derive(Subcommand)]
pub enum LiveTranscriptAction {
    /// Show capture setup and routing readiness (owner only)
    Status(TranscriptOwner),
    /// Read a page as the owner or the exact active consumer
    Read {
        #[command(flatten)]
        owner: TranscriptOwner,
        /// Active binding revision, required for tab credentials
        #[arg(long)]
        revision: Option<u64>,
        /// Sequence where the page starts
        #[arg(long)]
        from_seq: Option<u64>,
        /// Last sequence to return for a routed range
        #[arg(long)]
        through_seq: Option<u64>,
        /// Character offset from the previous Agent Format page
        #[arg(long, requires = "from_seq", conflicts_with = "through_seq")]
        char_offset: Option<usize>,
        /// Character budget for an Agent Format page
        #[arg(long, conflicts_with = "through_seq")]
        max_chars: Option<usize>,
    },
    /// Acknowledge a fully consumed committed sequence
    Ack {
        #[command(flatten)]
        owner: TranscriptOwner,
        #[command(flatten)]
        advance: TranscriptAdvance,
    },
    /// Mark a sequence seen without acknowledging consumption
    MarkSeen {
        #[command(flatten)]
        owner: TranscriptOwner,
        #[command(flatten)]
        advance: TranscriptAdvance,
    },
    /// Show the owner transcript's status, cursor and retained size
    Overview(TranscriptOwner),
    /// Search retained speech and summaries (owner only)
    Search {
        #[command(flatten)]
        owner: TranscriptOwner,
        /// Case-sensitive search text
        query: String,
        /// Sequence where the search starts
        #[arg(long)]
        from_seq: Option<u64>,
        /// Maximum rows to scan
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Select the attached router, an external tab, or the routing mode (owner only)
    Attach {
        #[command(flatten)]
        owner: TranscriptOwner,
        /// Exact external destination tab ID, including the owner tab itself
        #[arg(long)]
        external_tab: Option<String>,
        /// Set a deliberate trigger mode
        #[arg(long, value_parser = ["scout_then_concierge", "wake_word", "direct_batch", "manual"])]
        trigger: Option<String>,
        /// Set the trigger without replacing the active binding
        #[arg(long, requires = "trigger", conflicts_with_all = ["external_tab", "from_beginning"])]
        mode_only: bool,
        /// Start a newly selected external consumer from the beginning
        #[arg(long, requires = "external_tab")]
        from_beginning: bool,
    },
    /// Flush and pause an exact live capture lease (owner only)
    Pause {
        #[command(flatten)]
        owner: TranscriptOwner,
        /// Capture client UUID
        #[arg(long)]
        client: String,
        /// Capture lease UUID
        #[arg(long)]
        lease: String,
        /// Next capture frame sequence
        #[arg(long)]
        sequence: u32,
    },
    /// Admit a new capture lease in resume mode (owner only)
    Resume {
        #[command(flatten)]
        owner: TranscriptOwner,
        /// Capture client UUID that will provide the microphone frames
        #[arg(long)]
        client: String,
    },
}
