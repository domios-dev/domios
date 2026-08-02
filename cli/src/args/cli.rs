//! SOURCE OF TRUTH for the CodeTerm CLI surface.
//! On any change here run `codeterm cli-manifest --write` to regenerate
//! docs/cli-manifest.json + docs/CLI.md. The tests `cli_manifest_is_fresh`,
//! `docs_cli_md_is_fresh`, and `prompt_cli_commands_exist` (cli/src/cmd/manifest.rs)
//! plus the chat-ui `recognize.test.ts` fail CI if anything drifts.
//! See docs/contributing/cli.md.

use clap::{Parser, Subcommand};

use crate::mesh_cli::MeshAction;

use super::agent::{AgentAction, ReportArgs};
use super::cron::CronAction;
use super::debug::DebugAction;
use super::delayed::DelayedAction;
use super::effort::EffortAction;
use super::mem::MemAction;
use super::misc::{
    BridgeAction, ConfigAction, KeepAwakeAction, NotificationAction, PasteAction, TabAction,
    ThemeAction, WorkspaceAction,
};
use super::orchestration::OrchestrationAction;
use super::orchestrator::OrchestratorAction;
use super::plan::PlanAction;
use super::plugin::PluginAction;
use super::service::ServiceAction;
use super::sessions::SessionsAction;
use super::spec::SpecAction;
use super::system::SystemAction;
use super::task::{CanvasAction, TaskAction};
use super::team::TeamAction;
use super::update::UpdateAction;
use super::usage::UsageAction;

fn parse_doc_section(value: &str) -> Result<String, String> {
    if !value.chars().any(char::is_alphanumeric) {
        Err("section slug must contain a letter or number".to_string())
    } else {
        Ok(value.to_string())
    }
}

fn parse_doc_head(value: &str) -> Result<usize, String> {
    let lines = value
        .parse::<usize>()
        .map_err(|_| "head line count must be a positive integer".to_string())?;
    if lines == 0 {
        Err("head line count must be greater than zero".to_string())
    } else {
        Ok(lines)
    }
}

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
#[allow(
    clippy::large_enum_variant,
    reason = "Clap command enums keep parsed options inline."
)]
pub enum Commands {
    /// Show CodeTerm version and system info
    Identify,

    /// List CLI commands and clearly marked RPC-only/internal methods
    Capabilities,

    /// Plugin management — install, update, remove, manage channels
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },

    /// Dump the CLI command tree as JSON (source of truth for docs/prompts/cards)
    CliManifest {
        /// Regenerate committed artifacts (docs/cli-manifest.json + docs/CLI.md)
        #[arg(long)]
        write: bool,
        /// Emit the docs/CLI.md auto-block markdown instead of JSON
        #[arg(long, conflicts_with = "write")]
        markdown: bool,
    },

    /// Workspace management
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },

    /// Team registry and 1:1 plan binding
    Team {
        #[command(subcommand)]
        action: TeamAction,
    },

    /// Orchestrator helpers
    Orchestrator {
        #[command(subcommand)]
        action: OrchestratorAction,
    },

    /// Paired creation — spawn an Orchestrator + Planner bound to a fresh plan
    Orchestration {
        #[command(subcommand)]
        action: OrchestrationAction,
    },

    /// Tab management
    #[command(name = "tab")]
    Tab {
        #[command(subcommand)]
        action: TabAction,
    },

    /// Send text to a tab (default: current tab).
    ///
    /// Sending to a SHELL tab delivers raw text/keys verbatim — no agent
    /// return-address marker is injected, so the shell runs exactly what you
    /// send. Sending to an AGENT tab prepends the sender marker so the agent
    /// knows who to reply to. Key/sequence examples (use --actions):
    ///   --actions "up,up,enter"   replay last commands
    ///   --actions "ctrl-c"        interrupt
    ///   --actions "y"             answer a character prompt
    ///   --actions "esc"           dismiss
    ///   --actions "down*3,enter"  repeat with *N
    ///   "git status" --actions "enter"   text then submit
    #[command(name = "send")]
    SendText {
        /// Text to send (positional, or omit and use --from-file/--stdin)
        #[arg(allow_hyphen_values = true)]
        text: Option<String>,

        /// Target tab ID (default: $CODETERM_TAB_ID or focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,

        /// Read text from a file instead of the positional arg. Lets you
        /// pass payloads that would otherwise be eaten by shell quoting.
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,

        /// Read text from stdin. Pair with a single-quoted heredoc to bypass
        /// the shell entirely:
        ///   codeterm send --tab X --stdin <<'EOF'
        ///   any `backticks`, $vars, etc — zero interpolation
        ///   EOF
        ///
        /// send is fire-and-forget: the call returns as soon as bytes
        /// have been written to the PTY. For "wait for the agent to process
        /// my message", a future `codeterm agent send` primitive will hook
        /// into the provider's request/response cycle.
        #[arg(long, visible_alias = "from-stdin", conflicts_with = "text")]
        stdin: bool,

        /// Terminal actions to run after optional text, comma-separated.
        /// Keys: literal characters, enter, esc, tab, backspace, delete, up,
        /// down, left, right, home, end, pageup, pagedown, space, ctrl-c, plus
        /// delay:250ms / delay:1s pauses. Repeats: key*3 (e.g. down*3).
        /// With --actions, text is inserted first but not auto-submitted;
        /// include enter when you want to submit it.
        #[arg(long, conflicts_with_all = ["delay", "at"])]
        actions: Option<String>,

        /// Queue instead of sending immediately. Examples: 45s, 10m, 1h30m, 2d.
        /// Bare number means minutes. NOT the normal send path: use only for
        /// future delivery the user asked for — plain send is immediate and
        /// must never be buffered.
        #[arg(long, conflicts_with = "at")]
        delay: Option<String>,

        /// Queue for an absolute fire time: unix milliseconds, RFC3339, or local YYYY-MM-DDTHH:MM.
        #[arg(long, conflicts_with = "delay")]
        at: Option<String>,
    },

    /// Manage delayed Chat UI sends
    Delayed {
        #[command(subcommand)]
        action: DelayedAction,
    },

    /// Web mirror helpers (share links, etc.)
    Web {
        #[command(subcommand)]
        action: crate::args::misc::WebAction,
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

    /// Report worker progress to the owning orchestrator or plan
    Report(ReportArgs),

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
        /// Override manifest URL when paired with `install --auto`.
        /// Backend must be built with `dev-updater-allowlocal` for non-https URLs.
        #[arg(long)]
        manifest_url: Option<String>,
        /// Message to inject into the General Agent tab after the new daemon
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

    /// Canvas plans — persistent orchestration boards
    Plan {
        #[command(subcommand)]
        action: PlanAction,
    },

    /// Requirements specs — first-class WHAT documents
    Spec {
        #[command(subcommand)]
        action: SpecAction,
    },

    /// Complete work bundles and effort lineage
    Effort {
        #[command(subcommand)]
        action: EffortAction,
    },

    /// Standalone and plan-backed tasks
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },

    /// Canvas control commands
    Canvas {
        #[command(subcommand)]
        action: CanvasAction,
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

    /// Read embedded CodeTerm documentation. `codeterm docs` lists documents.
    /// For a long sectioned document, `codeterm docs <name>` prints a compact
    /// section index; `--section <slug>` returns one complete structural section
    /// and `--full` returns the byte-exact complete document. `--head <lines>` is
    /// an intentionally incomplete diagnostic excerpt, never an instruction-read
    /// workflow. Names and section slugs are case-insensitive.
    Docs {
        /// Doc name (e.g. CONFIG, API, MESH). Omit to list all available docs.
        name: Option<String>,

        /// Print the byte-exact complete document in one call.
        #[arg(long, requires = "name", conflicts_with_all = ["head", "section"])]
        full: bool,

        /// Print the first N source lines as a labelled incomplete diagnostic excerpt.
        #[arg(
            long,
            value_name = "LINES",
            value_parser = parse_doc_head,
            requires = "name",
            conflicts_with = "section"
        )]
        head: Option<usize>,

        /// Print one complete level-2 section, selected by normalized heading slug.
        #[arg(
            long,
            value_name = "SLUG",
            value_parser = parse_doc_section,
            requires = "name",
            conflicts_with_all = ["full", "head"]
        )]
        section: Option<String>,
    },

    /// Read one embedded CodeTerm document in full.
    ///
    /// Canonical recovery entry for agents: `codeterm doc orchestration`.
    Doc {
        /// Doc name (e.g. orchestration).
        name: String,
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
