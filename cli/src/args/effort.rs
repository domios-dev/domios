use clap::Subcommand;

#[derive(Subcommand)]
pub enum EffortAction {
    /// Show the Spec(s)+Plan+tasks bundle for one effort
    Show { plan_id: String },
    /// Link two efforts by lineage
    Link {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        rel: String,
    },
}
