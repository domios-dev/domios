use clap::Subcommand;

#[derive(Subcommand)]
pub enum ServiceAction {
    /// List all registered services (cron + daemon)
    List {
        /// Filter by kind: cron | daemon
        #[arg(long)]
        kind: Option<String>,
        /// Filter by enabled state
        #[arg(long)]
        enabled: Option<bool>,
    },
    /// Register a new daemon-kind service
    Add {
        name: String,
        /// Service kind (default: daemon — for cron use `codeterm cron create`)
        #[arg(long, default_value = "daemon")]
        kind: String,
        /// Health check mode: http | tcp | process | script
        #[arg(long, default_value = "http")]
        health_mode: String,
        /// HTTP URL (for health_mode=http)
        #[arg(long)]
        url: Option<String>,
        /// Host (for health_mode=tcp)
        #[arg(long)]
        host: Option<String>,
        /// Port (for health_mode=tcp)
        #[arg(long)]
        port: Option<u16>,
        /// Process label (for health_mode=process — launchd label or systemd unit)
        #[arg(long)]
        process_label: Option<String>,
        /// Script path (for health_mode=script — exit 0 = healthy)
        #[arg(long)]
        script_path: Option<String>,
        /// HTTP health path (default: /healthz)
        #[arg(long)]
        health_path: Option<String>,
        /// Probe interval in seconds (default: 30)
        #[arg(long, default_value = "30")]
        health_interval: u32,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Enable push API (daemon can write to panes via Bearer token)
        #[arg(long, default_value = "false")]
        push: bool,
    },
    /// Show service detail (status, config, recent activity)
    Show {
        /// Service ID
        id: String,
    },
    /// Edit a service
    Edit {
        /// Service ID
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        enabled: Option<bool>,
    },
    /// Delete a service (also deletes its push token + activity)
    Delete {
        /// Service ID
        id: String,
    },
    /// Show or rotate the push token for a service
    Token {
        /// Service ID
        id: String,
        /// Rotate (generate new token, invalidate old)
        #[arg(long)]
        rotate: bool,
    },
}
