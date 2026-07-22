use clap::Subcommand;

#[derive(Subcommand)]
pub enum SystemAction {
    /// Install an OS login trigger for the external crash-aware supervisor.
    /// The supervisor alone applies clean-exit, backoff, and loop policy;
    /// platform wrappers never restart the app independently.
    #[clap(name = "enable-autostart")]
    EnableAutostart {
        /// Override the path to the CodeTerm executable. Defaults to the
        /// installed app binary for the current platform.
        #[arg(long)]
        binary: Option<String>,
        /// Override the path to the codeterm CLI supervisor. Defaults to the
        /// currently-running CLI executable.
        #[arg(long)]
        supervisor: Option<String>,
    },
    /// Remove the OS-level autostart trigger created by `enable-autostart`.
    /// Idempotent — running on a host without a trigger is a no-op.
    #[clap(name = "disable-autostart")]
    DisableAutostart,
    /// Print whether the autostart trigger is installed and where.
    #[clap(name = "autostart-status")]
    AutostartStatus,
    /// Run the crash-aware external supervisor loop. Intended for OS wrappers.
    #[clap(name = "supervise", hide = true)]
    Supervise {
        /// Override the path to the CodeTerm executable. Defaults to the
        /// installed app binary for the current platform.
        #[arg(long)]
        binary: Option<String>,
        /// Run one launch/observe/classify cycle, then exit.
        #[arg(long)]
        once: bool,
    },
}
