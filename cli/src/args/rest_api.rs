use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum RestApiAction {
    /// Show listener phase and the last bind or runtime error over the local socket
    Status,
    /// Restart the REST listener and wait for readiness or a reported failure
    Restart,
}
