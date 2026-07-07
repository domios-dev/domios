use clap::Subcommand;

#[derive(Subcommand)]
pub enum SpecAction {
    /// Create a new spec
    Create {
        #[arg(long, allow_hyphen_values = true)]
        title: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// List specs
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        active: bool,
        #[arg(long)]
        archived: bool,
        #[arg(long)]
        all: bool,
    },
    /// Show a hydrated spec
    Show { spec_id: String },
    /// Delete a spec (scrubs plan references, removes the yaml)
    Delete {
        spec_id: String,
        /// Confirm permanent deletion.
        #[arg(long, alias = "confirm")]
        yes: bool,
    },
    /// Spec sections
    Section {
        #[command(subcommand)]
        action: SpecSectionAction,
    },
    /// Spec-to-plan links
    Link {
        #[command(subcommand)]
        action: SpecLinkAction,
    },
}

#[derive(Subcommand)]
pub enum SpecSectionAction {
    /// Add a spec section (requirement), optionally nested under --parent
    Add {
        spec_id: String,
        #[arg(long, allow_hyphen_values = true)]
        title: String,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        primary: Option<String>,
        #[arg(long)]
        handle: Option<String>,
    },
    /// Update a spec section's title, body, tags, or handle
    Update {
        spec_id: String,
        section_id: String,
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        primary: Option<String>,
        #[arg(long)]
        handle: Option<String>,
    },
    /// Show one spec section's title, body, tags, and subsections
    Show { spec_id: String, section_id: String },
    /// Remove a spec section and its subsections
    Rm { spec_id: String, section_id: String },
}

#[derive(Subcommand)]
pub enum SpecLinkAction {
    /// Link spec section(s) to plan section(s) (M:N coverage edges)
    Add {
        plan_id: String,
        #[arg(long)]
        spec: String,
        #[arg(long, requires = "plan_section")]
        spec_section: Option<String>,
        #[arg(long, requires = "spec_section")]
        plan_section: Option<String>,
        #[arg(
            long = "edge",
            value_name = "SPEC_SECTION:PLAN_SECTION",
            help = "Repeatable coverage edge; combine many in one call"
        )]
        edges: Vec<String>,
    },
    /// Remove spec-to-plan link(s); narrow with --spec-section / --plan-section
    Rm {
        plan_id: String,
        #[arg(long)]
        spec: String,
        #[arg(long)]
        spec_section: Option<String>,
        #[arg(long)]
        plan_section: Option<String>,
    },
    /// List spec-to-plan links for a spec
    List { spec_id: String },
}
