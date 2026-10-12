use clap::Subcommand;

#[derive(Subcommand)]
pub enum AnalyticsAction {
    /// Show recorded consent, queue size and delivery counters
    Status,
    /// Record opt-in consent and create an installation identifier
    Enable,
    /// Record opt-out consent and clear queued events and identifiers
    Disable,
}
