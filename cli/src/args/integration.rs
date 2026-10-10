use clap::Subcommand;

#[derive(Subcommand)]
pub enum IntegrationAction {
    /// Install a provider's hook integration and trust Domios's own hooks.
    /// Running it records consent the same way the Settings install does.
    Install {
        /// Provider id (e.g. claude, codex)
        #[arg(long)]
        provider: String,
    },
    /// Show a provider's hook integration state
    Status {
        /// Provider id (e.g. claude, codex)
        #[arg(long)]
        provider: String,
    },
    /// Preview dead provider launch artifacts; only the machine owner may remove them.
    SweepProfiles {
        /// Provider id (e.g. codex)
        #[arg(long)]
        provider: String,
        /// Remove the profiles listed by this instance's scan.
        #[arg(long)]
        confirm: bool,
        /// Include exact-pattern unmarked legacy profiles whose tabs are not live here.
        #[arg(long)]
        include_legacy: bool,
    },
}
