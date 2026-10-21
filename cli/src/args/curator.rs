use clap::Subcommand;

#[derive(Subcommand)]
pub enum CuratorAction {
    /// Validate the retro, record this round and choose the next cadence
    Finish {
        #[arg(long)]
        next: String,
        #[arg(long)]
        from_pad: String,
    },
}
