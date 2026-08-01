use clap::Subcommand;

#[derive(Subcommand)]
pub enum UsageAction {
    /// Latest usage snapshot (session + weekly %, reset times).
    Latest {
        #[arg(long, default_value = "claude")]
        provider: String,
        #[arg(long)]
        account_id: Option<String>,
    },
    /// End-of-today and end-of-cycle projection based on your historical
    /// rhythm scaled by today's observed pace. Plan-independent (the
    /// rhythm is normalized; today's % anchors the absolute scale).
    Projection {
        #[arg(long, default_value = "claude")]
        provider: String,
        #[arg(long)]
        account_id: Option<String>,
    },
    /// Re-read usage from the provider's own quota-free source (never spends quota).
    Refresh {
        #[arg(long, default_value = "claude")]
        provider: String,
        #[arg(long)]
        account_id: Option<String>,
    },
}
