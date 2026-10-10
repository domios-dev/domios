use clap::Subcommand;

#[derive(Subcommand)]
pub enum TraceAction {
    /// Show a retained flow or all flows involving a tab, with missing stages
    Show {
        id: String,
        /// Limit the retained window (for example 30m, 1h, 2d)
        #[arg(long)]
        since: Option<String>,
    },
    /// List the most recently observed flows
    Recent {
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=1000))]
        limit: u16,
    },
}
