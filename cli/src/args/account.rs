use clap::Subcommand;
#[derive(Subcommand)]
pub enum AccountAction {
    /// Sign in using the system browser and wait for completion
    Login,
    /// Show the current plan and stale-cache marker
    Plan,
    /// Open an upgrade checkout in the system browser
    Upgrade { plan_key: String },
    /// Open subscription management in the system browser
    Portal,
    /// Show the signed-in identity
    Status,
    /// Sign out and delete the saved credential
    Logout,
}
