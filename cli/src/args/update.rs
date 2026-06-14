use clap::Subcommand;

#[derive(Subcommand)]
pub enum UpdateAction {
    /// Check for available updates
    Check {
        /// Force re-check (ignore cache)
        #[arg(long)]
        force: bool,
    },
    /// Install update (--auto for automatic download + install + restart)
    Install {
        /// Use Tauri built-in updater: download, install, and restart automatically
        #[arg(long)]
        auto: bool,
        /// Override manifest URL (dev/test escape hatch). Requires the
        /// backend to be built with the `dev-updater-allowlocal` feature
        /// for non-https schemes; release builds reject it.
        #[arg(long)]
        manifest_url: Option<String>,
        /// Install from a local NSIS installer (Windows). Kills the running
        /// daemon, runs the installer silently, then relaunches codeterm.
        /// Bypasses network — useful for local-build testing.
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,
        /// Optional prompt to send to the calling pane once the new daemon
        /// is up — lets a session continue work post-restart.
        #[arg(long, value_name = "TEXT")]
        continue_prompt: Option<String>,
        /// Pane to deliver `--continue-prompt` to. Defaults to `$CODETERM_PANE_ID`.
        #[arg(long, value_name = "ID")]
        continue_pane: Option<String>,
        /// Message to inject into the General Agent pane after the new
        /// daemon boots. Lets the GA continue work post-update — e.g.
        /// "verify the new build and report version via Telegram".
        #[arg(long, value_name = "TEXT")]
        after_update: Option<String>,
    },
}
