use clap::Subcommand;

#[derive(Subcommand)]
pub enum SystemAction {
    /// Install an OS-level trigger that auto-launches CodeTerm at user
    /// login and re-launches it if the process disappears. Per platform:
    /// Windows = `schtasks` task with ONLOGON + 5-minute repeat,
    /// macOS = `~/Library/LaunchAgents/com.codeterm.app.plist` with
    /// `KeepAlive` + `RunAtLoad`, Linux = user systemd unit
    /// `~/.config/systemd/user/codeterm.service` with `Restart=on-failure`.
    /// Idempotent — running twice is a no-op.
    #[clap(name = "enable-autostart")]
    EnableAutostart {
        /// Override the path to the CodeTerm executable. Defaults to the
        /// installed app binary for the current platform.
        #[arg(long)]
        binary: Option<String>,
    },
    /// Remove the OS-level autostart trigger created by `enable-autostart`.
    /// Idempotent — running on a host without a trigger is a no-op.
    #[clap(name = "disable-autostart")]
    DisableAutostart,
    /// Print whether the autostart trigger is installed and where.
    #[clap(name = "autostart-status")]
    AutostartStatus,
}
