use clap::{Parser, Subcommand};

use crate::mesh_cli::MeshAction;

use super::agent::AgentAction;
use super::cron::CronAction;
use super::debug::DebugAction;
use super::mem::MemAction;
use super::service::ServiceAction;
use super::misc::{
    BridgeAction, ConfigAction, KeepAwakeAction, NotificationAction, PaneAction,
    PasteAction, ThemeAction, WorkspaceAction,
};
use super::sessions::SessionsAction;
use super::system::SystemAction;
use super::update::UpdateAction;
use super::usage::UsageAction;

#[derive(Parser)]
#[command(
    name = "codeterm",
    version,
    about = "CodeTerm CLI — control your terminal multiplexer",
    after_help = "Plugin management is available via `codeterm plugin --help`."
)]
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
        ///
        /// send-text is fire-and-forget: the call returns as soon as bytes
        /// have been written to the PTY. For "wait for the agent to process
        /// my message", a future `codeterm agent send` primitive will hook
        /// into the provider's request/response cycle.
        #[arg(long)]
        from_stdin: bool,
    },

    /// Cross-mesh ephemeral pastebin — stash payloads with short ids (#98)
    Paste {
        #[command(subcommand)]
        action: PasteAction,
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

    /// Manage daemon services (long-running processes monitored by codeterm)
    Service {
        #[command(subcommand)]
        action: ServiceAction,
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
        /// Message to inject into the General Agent pane after the new daemon
        /// boots. Useful for remote-triggered updates ("run the update and
        /// then verify health") — GA reads it as user input post-restart.
        #[arg(long, value_name = "TEXT")]
        after_update: Option<String>,
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

    /// Agent usage tracking — current %, projection for end-of-today /
    /// end-of-cycle based on your historical rhythm scaled to today's pace.
    Usage {
        #[command(subcommand)]
        action: UsageAction,
    },

    /// OS-level integration: autostart, supervision, install hooks. Each
    /// subcommand is local-only and writes to platform-specific places
    /// (schtasks on Windows, LaunchAgents on macOS, user systemd on Linux).
    System {
        #[command(subcommand)]
        action: SystemAction,
    },

    /// Read embedded CodeTerm documentation. Docs are bundled into the
    /// CLI binary at compile time so agents can request them on demand
    /// without bloating the system prompt. `codeterm docs` lists, `codeterm
    /// docs <name>` cats. Names are case-insensitive and match the .md
    /// stem (CONFIG, API, MESH, MEMORY, BUILD, PROMPTS, SKILLS, TESTING).
    Docs {
        /// Doc name (e.g. CONFIG, API, MESH). Omit to list all available docs.
        name: Option<String>,
    },

    /// Internal diagnostics — verify crash reporter, etc. Dev-only;
    /// hidden from --help because the matching daemon endpoint is gated to
    /// debug builds. Calling against a prod daemon returns Method-not-found.
    #[command(hide = true)]
    Debug {
        #[command(subcommand)]
        action: DebugAction,
    },
}
