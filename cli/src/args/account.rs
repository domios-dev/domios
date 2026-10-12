use clap::Subcommand;
#[derive(Subcommand)]
pub enum AccountAction {
    /// Sign in using the system browser and wait for completion
    Login,
    /// Show the signed-in identity
    Status,
    /// Sign out and delete the saved credential
    Logout,
}
