use clap::Subcommand;

#[derive(Subcommand)]
#[allow(
    clippy::large_enum_variant,
    reason = "Clap command enums keep parsed options inline."
)]
pub enum RadarAction {
    /// Add an item to this workspace's Radar; the same --source-provider/--source-id refreshes it
    Add {
        #[arg(long, allow_hyphen_values = true)]
        title: String,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long = "tag", allow_hyphen_values = true)]
        tag: Vec<String>,
        /// System the item came from, such as github or linear
        #[arg(long, value_name = "ID")]
        source_provider: Option<String>,
        /// The item's stable id in that system
        #[arg(long, value_name = "ID", allow_hyphen_values = true)]
        source_id: Option<String>,
        #[arg(long, value_name = "URL")]
        source_url: Option<String>,
        /// Workspace path; defaults to the current directory
        #[arg(long, value_name = "PATH")]
        project: Option<String>,
    },
    /// List this workspace's Radar items, newest first
    List {
        /// open (default), pulled or all
        #[arg(long, value_parser = ["open", "pulled", "all"])]
        status: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        /// Only items from this source provider
        #[arg(long)]
        provider: Option<String>,
        /// Case-insensitive text in the title or body
        #[arg(long, allow_hyphen_values = true)]
        query: Option<String>,
        /// Workspace path; defaults to the current directory
        #[arg(long, value_name = "PATH", conflicts_with = "all_workspaces")]
        project: Option<String>,
        /// List items of every workspace
        #[arg(long)]
        all_workspaces: bool,
    },
    /// Show one Radar item
    Show { item_id: String },
    /// Edit a Radar item's title, body or tags
    Edit {
        item_id: String,
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        /// Replace the tags; repeat for several
        #[arg(
            long = "tag",
            allow_hyphen_values = true,
            conflicts_with = "clear_tags"
        )]
        tag: Vec<String>,
        #[arg(long)]
        clear_tags: bool,
    },
    /// Remove a Radar item; a task it was pulled into stays in its plan
    Drop { item_id: String },
    /// Create a linked task for this item in a plan of the same workspace
    Pull {
        item_id: String,
        #[arg(long, value_name = "PLAN")]
        plan: String,
        /// Task kind; defaults to task
        #[arg(long = "type")]
        kind: Option<String>,
        /// Comma-separated section ids or handles
        #[arg(long)]
        sections: Option<String>,
    },
}
