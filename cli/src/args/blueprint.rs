use clap::Subcommand;

#[derive(Subcommand)]
pub enum BlueprintAction {
    /// Discover repository Blueprints without running, applying or approving them
    Project {
        #[command(subcommand)]
        action: BlueprintProjectAction,
    },
    /// List built-in graph skeletons or print one ready-to-edit recipe
    Recipe { name: Option<String> },
    /// Validate a YAML file or built-in recipe without a daemon
    Check {
        source: String,
        #[arg(long = "set")]
        set: Vec<String>,
        /// Include positioned atoms and proof trees in human-readable diagnostics
        #[arg(long)]
        explain: bool,
        /// Print deterministic safe repairs and the semantic diff
        #[arg(long)]
        fix: bool,
        /// With --fix, write the repaired YAML file in place
        #[arg(long, requires = "fix")]
        write: bool,
    },
    /// Create or update a YAML file or recipe by name in the caller's Team
    #[command(alias = "create")]
    Apply {
        #[arg(default_value = "")]
        source: String,
        /// Offer proven graphs matching this intent before authoring
        #[arg(long, conflicts_with_all = ["source", "set", "note", "dry_run"])]
        like: Option<String>,
        #[arg(long = "set")]
        set: Vec<String>,
        #[arg(long, value_parser = parse_blueprint_note)]
        note: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Print a commented YAML scaffold from a recipe or node kind
    New {
        #[arg(default_value = "watch")]
        kind: String,
        /// Offer proven graphs matching this intent before authoring
        #[arg(long, conflicts_with = "kind")]
        like: Option<String>,
    },
    /// Keep a generalised solution (Team Orchestrator only); give 3-5 --when intents
    Keep {
        #[arg(required_unless_present = "file", conflicts_with = "file")]
        id: Option<String>,
        /// Save a triggerless typed callable without scheduling it
        #[arg(long, conflicts_with = "id")]
        file: Option<std::path::PathBuf>,
        /// One "when would someone want this" request; repeat 1-5 times, 3-5 phrased differently
        #[arg(long, required = true, action = clap::ArgAction::Append)]
        when: Vec<String>,
    },
    /// Print the authoring JSON Schema for editors and agents
    Schema,
    /// Export a portable recipe containing only the definition
    Export {
        id: String,
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Import a native v2 recipe into the caller's Team (stopped by default)
    Import {
        file: std::path::PathBuf,
        #[arg(long = "set")]
        set: Vec<String>,
        #[arg(long, conflicts_with = "start")]
        stopped: bool,
        /// Start immediately after validation
        #[arg(long, conflicts_with = "stopped")]
        start: bool,
    },
    /// List trigger, step, converter and event-source capabilities
    Nodes,
    /// List blueprints scoped to the calling tab and its Team
    List,
    /// Show one blueprint
    Show { id: String },
    /// Deliver a text body to a running text input stage
    #[command(group = clap::ArgGroup::new("text_source").args(["from_pad", "text_file"]).required(true))]
    Input {
        id: String,
        /// Read a UTF-8 body from the caller's Agent Pad
        #[arg(long)]
        from_pad: Option<String>,
        /// Read a UTF-8 body from a file
        #[arg(long)]
        text_file: Option<std::path::PathBuf>,
    },
    /// Stop one blueprint
    Stop { id: String },
    /// Delete a whole blueprint, or every stopped one in the Team; the creator or the Team owner may delete
    #[command(group = clap::ArgGroup::new("delete_target").args(["id", "stopped"]).required(true))]
    Delete {
        id: Option<String>,
        /// Delete every stopped blueprint the caller may delete in its Team
        #[arg(long, conflicts_with_all = ["id", "stop"])]
        stopped: bool,
        /// Stop a running blueprint before deleting it
        #[arg(long, requires = "id")]
        stop: bool,
    },
    /// Replace one native v2 YAML definition at the expected version
    Update {
        /// Replace the Blueprint note (at most 200 plain-text characters)
        #[arg(long, value_parser = parse_blueprint_note)]
        note: Option<String>,
        id: String,
        #[arg(long)]
        expect_version: u64,
        #[arg(long)]
        from_file: std::path::PathBuf,
        #[arg(long)]
        cause: Option<String>,
        /// Validate and print the resulting graph without updating it
        #[arg(long)]
        dry_run: bool,
    },
    /// Show immutable definition versions
    History { id: String },
    /// List bounded run journal entries, newest first
    Log {
        id: String,
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..=200))]
        limit: u16,
    },
    /// Resolve an open escalation
    Escalation {
        #[command(subcommand)]
        action: BlueprintEscalationAction,
    },
}

#[derive(Subcommand)]
pub enum BlueprintProjectAction {
    /// List source path, name and intent under .domios/blueprints/
    List,
    /// View a native v2 source; no execution or approval is performed
    Show { path: String },
}

#[derive(Subcommand)]
pub enum BlueprintEscalationAction {
    /// Release a held stage and re-evaluate its observation
    Resolve { id: String, transition_id: String },
}

#[cfg(test)]
mod tests;

fn parse_blueprint_note(value: &str) -> Result<String, String> {
    codeterm_blueprint_types::automation_name::validate_note(Some(value))?;
    Ok(value.into())
}
