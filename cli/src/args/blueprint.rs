use clap::Subcommand;

#[derive(Subcommand)]
pub enum BlueprintAction {
    /// Manage the per-graph webhook credential; secrets appear only with --reveal
    Webhook {
        #[command(subcommand)]
        action: BlueprintWebhookAction,
    },
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
    #[command(
        after_help = "A run value is an author-fixed shell command line, not an argv array. macOS/Linux use /bin/sh -c; Windows uses powershell.exe -NoLogo -NoProfile -NonInteractive -Command. Shell quoting and expansion apply, including $(...) in double quotes. Runtime bind.in data is delivered in the JSON stdin envelope, never interpolated into run."
    )]
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
    /// Start from intent recall cards and a native v2 scaffold, or a named recipe
    New {
        #[arg(default_value = "watch")]
        kind: String,
        /// Offer proven graphs matching this intent before authoring
        #[arg(long, conflicts_with = "kind")]
        like: Option<String>,
        /// Recall scoped typed solutions before authoring; bodies open with blueprint show
        #[arg(long, conflicts_with_all = ["kind", "like"], value_parser = parse_blueprint_intent)]
        intent: Option<String>,
        /// Maximum recall cards (1..20)
        #[arg(long, requires = "intent", default_value_t = 3, value_parser = clap::value_parser!(u16).range(1..=20))]
        limit: u16,
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
        /// Choose 1-3 distinct tags from `blueprint tags`; repeat --tag for each
        #[arg(long = "tag", required = true, action = clap::ArgAction::Append)]
        tags: Vec<String>,
    },
    /// List controlled saved-solution tags with descriptions
    Tags,
    /// Print the authoring JSON Schema for editors and agents
    Schema,
    /// Export a portable recipe containing only the definition
    Export {
        id: String,
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Import a native v2 recipe into the caller's scope (stopped by default)
    Import {
        file: std::path::PathBuf,
        /// Destination: general, workspace:<id>, or team:<id>; caller must belong to it
        #[arg(long)]
        scope: Option<String>,
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
pub enum BlueprintWebhookAction {
    /// Generate and store a new webhook credential
    Enable {
        id: String,
        /// Print the newly generated secret once to stdout
        #[arg(long)]
        reveal: bool,
    },
    /// Replace the credential, immediately invalidating the previous secret
    Rotate {
        id: String,
        /// Print the newly generated secret once to stdout
        #[arg(long)]
        reveal: bool,
    },
    /// Remove the webhook credential
    Disable { id: String },
    /// Print the webhook URL without revealing a credential
    ShowUrl { id: String },
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

fn parse_blueprint_intent(value: &str) -> Result<String, String> {
    if value.trim().is_empty() || value.len() > 4096 || value.contains('\0') {
        return Err("Intent must contain 1..4096 UTF-8 bytes without NUL".into());
    }
    Ok(value.into())
}
