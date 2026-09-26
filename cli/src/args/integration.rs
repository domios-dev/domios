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
        #[arg(long, conflicts_with = "no_precompact_memory_export")]
        precompact_memory_export: bool,
        /// Disable conversation export before compaction
        #[arg(long, conflicts_with = "precompact_memory_export")]
        no_precompact_memory_export: bool,
    },
    /// Show a provider's hook integration state
    Status {
        /// Provider id (e.g. claude, codex)
        #[arg(long)]
        provider: String,
    },
    /// Preview or remove dead provider launch artifacts owned by this instance.
    SweepProfiles {
        /// Provider id (e.g. codex)
        #[arg(long)]
        provider: String,
        /// Remove the profiles listed by this instance's scan.
        #[arg(long)]
        confirm: bool,
        /// Include every unmarked legacy profile, regardless of tab liveness.
        #[arg(long)]
        include_legacy: bool,
    },
}
