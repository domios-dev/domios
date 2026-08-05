use clap::Subcommand;

#[derive(Subcommand)]
pub enum PadAction {
    /// Print the current agent tab's private ephemeral pad path
    Path,
    /// List relative regular files in the current agent tab's pad
    List,
}
