use clap::Subcommand;

#[derive(Subcommand)]
pub enum SessionsAction {
    /// Search past sessions by content across all registered providers.
    /// Results are tagged with `provider` so an agent can resume with the
    /// right launcher.
    Search {
        /// Query string. Terms are whitespace-split; every term must
        /// appear (case-insensitive) in at least one matching message.
        query: String,
        /// Filter by provider id (e.g. "claude"). Omit or pass "all" to
        /// search every registered provider.
        #[arg(long)]
        provider: Option<String>,
        /// Restrict to sessions tagged with this project path. Handy when
        /// you only want hits from the current repo.
        #[arg(long)]
        project: Option<String>,
        /// Max sessions returned.
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Max snippets shown per session.
        #[arg(long, default_value = "3")]
        max_snippets: usize,
    },
    /// Resume a past session in its original cwd. Atomic replacement for
    /// the "workspace create → agent spawn --args=--resume=UUID" dance
    /// which keeps failing because Claude/Codex look for the JSONL in the
    /// current cwd. This command: finds the session across providers,
    /// creates a workspace at the right project path, spawns an agent
    /// tab there, and hands the resume id to the provider correctly.
    Resume {
        /// Session id to resume (UUID).
        session_id: String,
        /// Tab title for the new tab. Defaults to "resumed <shortid>".
        #[arg(long)]
        title: Option<String>,
        /// Reuse an existing workspace instead of creating a new one.
        #[arg(long)]
        workspace_id: Option<String>,
        /// Custom name for the new workspace (default: basename of cwd).
        #[arg(long)]
        workspace_name: Option<String>,
        /// Model override; translated to the provider's flag.
        #[arg(long)]
        model: Option<String>,
    },
    /// Print metadata for a past session (provider, cwd, file path, times).
    /// Read-only probe — use before `sessions resume` to verify the session
    /// is still on disk.
    Get { session_id: String },
    /// Full-desk session backups (workspaces + tabs). List the rotated
    /// snapshots and recover a reduced/wiped desk from one without
    /// hand-editing JSON.
    Backups {
        #[command(subcommand)]
        action: BackupsAction,
    },
}

#[derive(Subcommand)]
pub enum BackupsAction {
    /// List desk snapshot backups, newest first, with workspace/tab counts.
    List {
        /// Max backups shown.
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Restore a backup into the live desk. Strictly additive — it replaces
    /// NOTHING: existing workspaces and tabs are untouched, only workspaces
    /// and tabs missing from the live desk are recreated from the backup.
    Restore {
        /// Backup name from `sessions backups list` (unix timestamp).
        name: String,
    },
}
