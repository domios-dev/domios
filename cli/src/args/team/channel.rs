use clap::Subcommand;

#[derive(Subcommand)]
pub enum TeamChannelAction {
    /// Subscribe to a team channel and fetch its packages once
    Add {
        url: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// List team channels and their last refresh outcomes
    List,
    /// Explicitly refresh one channel, or all subscribed channels
    Refresh { name: Option<String> },
    /// Remove a community team channel
    Remove { name: String },
}
