use clap::Subcommand;

#[derive(Subcommand)]
pub enum EffortAction {
    /// Show the full effort bundle (linked spec(s) + plan + tasks) for one plan_id
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
