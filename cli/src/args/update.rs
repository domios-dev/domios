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
        /// Install from a local NSIS installer (Windows) or app bundle/DMG
        /// (macOS), then relaunch Domios. Bypasses the network.
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,
        /// Optional prompt to send to the calling tab's own session after restart.
        #[arg(long, value_name = "TEXT")]
        continue_prompt: Option<String>,
        /// Calling tab to continue. Must match `$CODETERM_TAB_ID`.
        #[arg(long = "continue-tab", value_name = "ID")]
        continue_tab: Option<String>,
        /// Suppress the calling tab's continuation for the next start.
        #[arg(long, conflicts_with_all = ["continue_prompt", "continue_tab"])]
        no_continue: bool,
        /// Message to inject into the General Agent tab after the new
        /// daemon boots. Lets the GA continue work post-update — e.g.
        /// "verify the new build and report version via Telegram".
        #[arg(long, value_name = "TEXT")]
        after_update: Option<String>,
    },
}
