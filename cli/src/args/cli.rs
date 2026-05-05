use clap::{Parser, Subcommand};

use crate::mesh_cli::MeshAction;

use super::agent::AgentAction;
use super::cron::CronAction;
use super::mem::MemAction;
use super::misc::{
    BridgeAction, BrowserAction, ConfigAction, KeepAwakeAction, NotificationAction, PaneAction,
    ThemeAction, WorkspaceAction,
};
use super::sessions::SessionsAction;
use super::update::UpdateAction;

#[derive(Parser)]
#[command(name = "codeterm", version, about = "CodeTerm CLI — control your terminal multiplexer")]
pub struct Cli {
    /// Output raw JSON instead of human-readable text
    #[arg(long, global = true)]
    pub json: bool,

    /// Execute command on a remote mesh peer instead of locally
    #[arg(long, global = true)]
    pub mesh: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Show CodeTerm version and system info
    Identify,

    /// List all available API methods
    Capabilities,

    /// Workspace management
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },

    /// Pane management
    Pane {
        #[command(subcommand)]
        action: PaneAction,
    },

    /// Get terminal output from a pane (default: current pane)
    GetOutput {
        /// Target pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,

        /// Maximum number of lines to return (approximate)
        #[arg(long, default_value = "50")]
        lines: u32,
    },

    /// Send text to a pane (default: current pane)
    SendText {
        /// Text to send (positional, or omit and use --from-file/--from-stdin)
        #[arg(default_value = "")]
        text: String,

        /// Target pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,

        /// Read text from a file instead of the positional arg. Lets you
        /// pass payloads that would otherwise be eaten by shell quoting.
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,

        /// Read text from stdin. Pair with a single-quoted heredoc to bypass
        /// the shell entirely:
        ///   codeterm send-text --pane X --from-stdin <<'EOF'
        ///   any `backticks`, $vars, etc — zero interpolation
        ///   EOF
        #[arg(long)]
        from_stdin: bool,
    },

    /// Open a canvas panel (localhost, data URLs, HTML visualizations)
    Browser {
        #[command(subcommand)]
        action: BrowserAction,
    },

    /// Send a notification
    Notify {
        /// Notification title
        title: String,

        /// Notification body
        #[arg(long)]
        body: Option<String>,

        /// Notification level: info, warning, error
        #[arg(long, default_value = "info")]
        level: String,
    },

    /// List or manage notifications
    Notifications {
        #[command(subcommand)]
        action: Option<NotificationAction>,

        /// Show only unread notifications
        #[arg(long)]
        unread: bool,
    },

    /// Prevent the system from sleeping while an agent is running
    Keepawake {
        #[command(subcommand)]
        action: Option<KeepAwakeAction>,
    },

    /// Theme management
    Theme {
        #[command(subcommand)]
        action: ThemeAction,
    },

    /// Agent hierarchy management
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },

    /// Cron job management
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },

    /// Check for updates and manage CodeTerm version
    Update {
        #[command(subcommand)]
        action: Option<UpdateAction>,
        /// Shortcut for `update check`
        #[arg(long, conflicts_with_all = ["apply", "channel"])]
        check: bool,
        /// Shortcut for `update install --auto`
        #[arg(long, conflicts_with_all = ["check", "channel"])]
        apply: bool,
        /// Shortcut for `update channel [name]`. Pass without value to print current channel.
        #[arg(long, num_args = 0..=1, default_missing_value = "", conflicts_with_all = ["check", "apply"])]
        channel: Option<String>,
        /// Override manifest URL when paired with --apply (or `install --auto`).
        /// Backend must be built with `dev-updater-allowlocal` for non-https URLs.
        #[arg(long)]
        manifest_url: Option<String>,
    },

    /// Headless self-update flow: download → swap → verify → roll back on
    /// mismatch. Survives daemon shutdown via an external orchestrator
    /// script (schtasks on Windows). Routes the result through the
    /// Telegram bridge once the new daemon boots.
    SelfUpdate {
        /// Skip download and just print whether an update is available.
        #[arg(long, conflicts_with_all = ["apply", "rollback"])]
        check: bool,
        /// Run the full upgrade flow.
        #[arg(long, conflicts_with_all = ["check", "rollback"])]
        apply: bool,
        /// Restore the previous binary from `.bak` and restart.
        #[arg(long, conflicts_with_all = ["check", "apply"])]
        rollback: bool,
        /// Channel override for `--apply`. Defaults to the configured channel.
        #[arg(long)]
        channel: Option<String>,
    },

    /// Read or write CodeTerm configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Send a message to an external service (Telegram, etc.)
    Bridge {
        #[command(subcommand)]
        action: BridgeAction,
    },

    /// Mesh networking — peer discovery, pairing, relay
    Mesh {
        #[command(subcommand)]
        action: MeshAction,
    },

    /// Unified memory store — skills, facts, notes, scripts, peers, preferences
    Mem {
        #[command(subcommand)]
        action: MemAction,
    },

    /// Past agent sessions — search conversations across all providers
    Sessions {
        #[command(subcommand)]
        action: SessionsAction,
    },

    /// Run a command and filter its output through per-tool truncation
    /// patterns. Reduces tokens spent reading test/build/log output by
    /// keeping diagnostic signal lines and dropping known noise (passing
    /// tests, compile spam, hunk middles).
    Snip {
        /// Command and arguments to run, including any leading `KEY=val`
        /// env prefixes (e.g. `RUST_LOG=debug cargo test`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true, num_args = 1..)]
        cmd: Vec<String>,
    },
}
