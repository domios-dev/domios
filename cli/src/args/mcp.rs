use clap::Subcommand;

#[derive(Subcommand)]
pub enum McpAction {
    /// Serve tab-scoped Domios tools over MCP stdio using inherited authority
    Serve {
        #[arg(long, conflicts_with = "mesh")]
        tab: String,
        #[arg(long)]
        binding_nonce: String,
    },
}
