use clap::Subcommand;

#[derive(Subcommand)]
pub enum MapAction {
    /// Build or refresh the project map for a workspace.
    Index {
        /// Workspace ID. Default: walk up from cwd.
        #[arg(long)]
        workspace: Option<String>,
        /// Ignore the mtime cache and re-extract every file.
        #[arg(long)]
        full: bool,
        /// Print per-file progress to stderr.
        #[arg(long)]
        verbose: bool,
    },
    /// Print last-index time, node/edge counts, errors.
    Status {
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Search nodes by display name (substring match).
    Search {
        query: String,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long, default_value = "20")]
        limit: u32,
        /// Filter by node kind (function, method, class, file, …).
        #[arg(long)]
        kind: Option<String>,
    },
}
