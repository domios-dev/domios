use clap::Subcommand;
#[derive(Subcommand)]
pub enum AppAction {
    /// Quit the exact local application after successful server cleanup and observed exits
    Quit {
        #[arg(long, required = true)]
        exact: bool,
    },
}
