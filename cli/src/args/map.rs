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
    /// Read or write per-workspace map configuration.
    Config {
        #[command(subcommand)]
        action: MapConfigAction,
    },
    /// Export the graph (nodes + edges) as JSON.
    Export {
        #[arg(long)]
        workspace: Option<String>,
        /// Skip workspace resolution and open the index at this absolute path.
        /// Daemon uses this to avoid a socket round-trip when serving graph queries.
        #[arg(long)]
        workspace_root: Option<String>,
        /// Restrict output to the k-hop subgraph centered on this node ID.
        #[arg(long)]
        node: Option<i64>,
        /// k-hop depth for `--node`. Default: 1.
        #[arg(long, default_value = "1")]
        k_hops: u32,
        /// Maximum nodes to return in the full-graph case.
        #[arg(long, default_value = "5000")]
        node_cap: usize,
    },
}

#[derive(Subcommand)]
pub enum MapConfigAction {
    /// Print the configured value for a key, or the whole config.
    Get {
        #[arg(long)]
        workspace: Option<String>,
        /// Specific key (e.g. `indexing.enabled`).
        #[arg(long)]
        key: Option<String>,
    },
    /// Set a key to a value. Value parses as bool / int as appropriate.
    Set {
        /// Configuration key (`indexing.enabled`, `watch.debounce_ms`, `index.max_files`).
        key: String,
        /// New value.
        value: String,
        #[arg(long)]
        workspace: Option<String>,
    },
}
