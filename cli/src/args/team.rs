use clap::Subcommand;

pub mod exchange;
pub mod template_start;

#[derive(Subcommand)]
pub enum TeamAction {
    /// List registered Teams
    List,
    /// Show one Team
    Show {
        /// Team ID
        team_id: String,
    },
    /// Give a Team a human-readable name
    Rename {
        /// Team ID
        team_id: String,
        /// New Team name
        name: String,
    },
    /// Close every member tab and remove the Team registry entry. Refused while
    /// a Team task is in progress or a Worker is working, unless --force.
    Close {
        /// Team ID
        team_id: String,
        /// Close anyway: reports every killed Worker and task, and keeps plans
        /// with unfinished tasks active and flagged
        #[arg(long)]
        force: bool,
    },
    /// Bind an existing plan to a Team
    AdoptPlan {
        /// Team ID
        team_id: String,
        /// Plan ID
        plan_id: String,
    },
    /// Attach a running roleless agent or shell locally, or adopt an unbound remote worker with --peer
    AdoptAgent {
        /// Team ID
        team_id: String,
        /// Tab ID of the running agent or shell
        tab_id: String,
        /// Peer that owns the running remote worker; the Team stays local
        #[arg(long)]
        peer: Option<String>,
    },
    /// Attach the one optional Cartographer Manager to a Team
    AttachCartographer {
        /// Team ID
        team_id: String,
        /// Agent provider ID
        #[arg(long)]
        provider: String,
        /// Provider model ID
        #[arg(long)]
        model: Option<String>,
        /// UTF-8 brief file, or - to read stdin; delivered to a new or existing Cartographer
        #[arg(long)]
        brief_file: Option<String>,
    },
    /// Create an Orchestrator + Planner Team in one action. The Plan stays
    /// unbound until the first goal.
    Pair {
        /// Orchestrator provider, optionally with a model (`provider[:model]`)
        #[arg(long)]
        orchestrator: String,
        /// Planner provider, optionally with a model (`provider[:model]`)
        #[arg(long)]
        planner: String,
        #[arg(long, env = "CODETERM_WORKSPACE_ID")]
        workspace: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        /// Explicit "I know I am a leaf and I am doing this deliberately"
        /// declaration — required only when the calling tab holds a Manager
        /// or Worker role. Creation is then attributed to this tab, never to
        /// an intermediary shell.
        #[arg(long)]
        acknowledge_leaf: bool,
        /// Start from this team template (`name[@version]`); omitted keeps the standard pair
        #[arg(long)]
        template: Option<String>,
    },
    /// Validate an ATP team package: a `name.team.yaml` file, a `name.team` archive or a package folder
    Check {
        /// Package path
        path: String,
    },
    /// Team templates: list, preview and start
    Template {
        #[command(subcommand)]
        action: template_start::TemplateAction,
    },
    /// Distill a live Team into a checked template package (procedure: `domios docs TEAMS`)
    Distill(exchange::DistillArgs),
    /// Export a template as `yaml`, `team` or `folder`
    Export(exchange::ExportArgs),
    /// Import a template package into My templates
    Import(exchange::ImportArgs),
}

#[cfg(test)]
mod tests;
