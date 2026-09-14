use clap::Subcommand;

#[derive(Subcommand)]
pub enum IntegrationAction {
    /// Install a provider's hook integration and trust CodeTerm's own hooks.
    /// Running it records consent the same way the Settings install does.
    Install {
        /// Provider id (e.g. claude, codex)
        #[arg(long)]
        provider: String,
        /// Also export conversation content to memory before compaction
        #[arg(long)]
        precompact_memory_export: bool,
    },
    /// Show a provider's hook integration state
    Status {
        /// Provider id (e.g. claude, codex)
        #[arg(long)]
        provider: String,
    },
}
