use clap::Subcommand;

#[derive(Subcommand)]
pub enum WorkspaceAction {
    /// List all workspaces
    List,
    /// Print the current (focused) workspace ID
    Current,
    /// Create a new workspace
    Create {
        /// Workspace name
        name: String,
        /// Working directory — agents spawned with --workspace inherit this as their CWD
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Focus a workspace by ID
    Focus {
        /// Workspace ID
        id: String,
    },
    /// Rename a workspace
    Rename {
        /// Workspace ID
        id: String,
        /// New name
        name: String,
    },
    /// Close/delete a workspace
    Close {
        /// Workspace ID
        id: String,
    },
    /// Move a tab to another workspace
    MoveTab {
        /// Tab ID to move
        tab_id: String,
        /// Source workspace ID
        from: String,
        /// Destination workspace ID
        to: String,
    },
}

#[derive(Subcommand)]
pub enum PaneAction {
    /// List all panes as a workspace-grouped tree. `--agents` filters to
    /// agent panes only (replaces the old `agent list`).
    List {
        /// Show only panes running an agent (have an agent_type).
        #[arg(long)]
        agents: bool,
    },
    /// Plain shell pane. For an AI pane use `codeterm agent spawn` instead.
    New {
        /// Target workspace id (default: current focused)
        #[arg(long)]
        workspace: Option<String>,
        /// Working directory for the new pane
        #[arg(long)]
        cwd: Option<String>,
        /// Tab title
        #[arg(long)]
        title: Option<String>,
        /// Command to run on pane start (e.g. `"poetry run backend"`).
        /// Sent into the PTY after the shell is ready.
        #[arg(long)]
        cmd: Option<String>,
    },
    /// Print the current (focused) pane ID — no JSON parsing needed
    Current {
        /// Return the last pane instead of the focused one
        #[arg(long)]
        last: bool,
    },
    /// Focus a pane by ID
    Focus {
        /// Pane ID
        id: String,
    },
    /// Close a pane by ID
    Close {
        /// Pane ID
        id: String,
    },
    /// Rename a pane tab (default: current pane)
    Rename {
        /// New title
        title: String,
        /// Pane ID to rename (default: current focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,
    },
    /// Get visible terminal text of a pane (what the user sees)
    #[command(alias = "get_text", alias = "screen")]
    GetText {
        /// Pane ID (default: current focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,
        /// Include raw ANSI escape codes (colors, cursor state) instead of plain text
        #[arg(long)]
        formatted: bool,
    },
    /// Execute a command in a pane and return output
    Exec {
        /// Command to execute
        command: String,
        /// Pane ID (default: current focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,
        /// Timeout in milliseconds
        #[arg(long, default_value = "10000")]
        timeout: u64,
    },
    /// Show agent status for a pane (type, session, prompt)
    Status {
        /// Pane ID (default: current focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,
    },
    /// Fuzzy search panes by id-prefix, tab title, friendly session name, pane title,
    /// agent type, or current model. Cross-script — "Далтон" matches "Dalton". Add
    /// `--mesh PEER` to search a paired peer (#99). `find` kept as alias.
    #[command(alias = "find")]
    Search {
        /// Substring or short pattern to match
        pattern: String,
    },
    /// Interrupt the agent's current generation (NOT a clean exit). Sends the
    /// provider's declared stop_keystroke — Esc for Claude/Codex/OpenCode,
    /// Ctrl+C for Cursor. Plain shells get a 200 with {stopped:false} (no key).
    /// Designed for mesh/CLI/bot drivers — type `/stop` in a Telegram bridge
    /// and have it map straight here.
    Stop {
        /// Pane ID (default: current focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,
    },

    /// Read a pane's parsed chat transcript (text/thinking/tool calls/results)
    Chat {
        /// Target pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,
        /// Only return messages from this index onward
        #[arg(long, default_value = "0")]
        since: usize,
    },
}

/// Cross-mesh ephemeral pastebin (#98). In-memory; daemon restart wipes
/// every paste. 20-entry cap, oldest-first eviction. Use for handing
/// payloads between machines: stash a blob with `paste put --mesh PEER`,
/// reference the slug in the next CLI call, let the peer `paste pop` it.
#[derive(Subcommand)]
pub enum PasteAction {
    /// Stash a payload. Body from positional / --from-file / stdin.
    Put {
        /// Body (positional). Omit and use --from-file or --from-stdin.
        #[arg(default_value = "")]
        body: String,
        /// Read body from a file instead.
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,
        /// Read body from stdin.
        #[arg(long)]
        from_stdin: bool,
        /// TTL — e.g. "30m", "1h", "24h". Default 1h. Max 24h.
        #[arg(long)]
        ttl: Option<String>,
        /// MIME hint (default text/plain).
        #[arg(long)]
        content_type: Option<String>,
        /// One-shot — paste self-deletes after first fetch.
        #[arg(long)]
        pop: bool,
    },
    /// Print a paste body to stdout. Does NOT delete unless --pop or the
    /// paste was created with --pop.
    Get {
        /// Paste id (e.g. "paste:a3f9k2").
        id: String,
        /// Delete after fetching (one-shot read).
        #[arg(long)]
        pop: bool,
    },
    /// Get + delete in one call. Sugar for `paste get <id> --pop`.
    Pop {
        /// Paste id.
        id: String,
    },
    /// List local (or --mesh <peer>) pastes — id, size, ttl_remaining.
    List,
    /// Explicit delete.
    Rm {
        /// Paste id.
        id: String,
    },
}

#[derive(Subcommand)]
pub enum NotificationAction {
    /// Clear all notifications
    Clear,
}

#[derive(Subcommand)]
pub enum KeepAwakeAction {
    /// Show current keep-awake status (default)
    Status,
    /// Start keeping the system awake
    Start {
        /// Duration in seconds (omit for indefinite)
        #[arg(long, alias = "duration")]
        duration_secs: Option<u64>,
        /// Also prevent display sleep
        #[arg(long)]
        display: bool,
    },
    /// Stop the current keep-awake session
    Stop,
}

#[derive(Subcommand)]
pub enum ThemeAction {
    /// List available themes
    List,
    /// Apply a theme by name
    Apply {
        /// Theme name (builtin or custom)
        name: String,
    },
    /// Save a custom theme from a TOML file
    Save {
        /// Theme ID (filename without .toml)
        id: String,
        /// Path to theme TOML file
        file: String,
        /// Apply the theme immediately after saving
        #[arg(long)]
        apply: bool,
    },
    /// Delete a custom theme
    Delete {
        /// Theme ID to delete
        id: String,
    },
    /// Show the custom themes directory path
    Dir,
}

#[derive(Subcommand)]
pub enum BridgeAction {
    /// Start or restart messaging bridge (Telegram, etc.)
    Restart,
    /// Send a message to Telegram or other service
    Send {
        /// Message text (also used as caption for media types)
        text: Option<String>,
        /// Target service
        #[arg(long, default_value = "telegram")]
        service: String,
        /// Recipient chat/user ID
        #[arg(long)]
        to: i64,
        /// Append to previous message instead of replacing (edits existing message)
        #[arg(long)]
        append: bool,
        /// Content type: text, photo, document, voice, animation, video, sticker
        #[arg(long, default_value = "text", rename_all = "lowercase")]
        r#type: String,
        /// File path for media types
        #[arg(long)]
        file: Option<String>,
        /// Caption for media (alternative to positional text)
        #[arg(long)]
        caption: Option<String>,
        /// Override filename for document uploads
        #[arg(long)]
        filename: Option<String>,
        /// Inline keyboard buttons as JSON: [[{"text":"Yes","data":"yes"},{"text":"No","data":"no"}]]
        #[arg(long)]
        buttons: Option<String>,
    },
    /// Set an emoji reaction on a message
    React {
        /// Recipient chat/user ID
        #[arg(long)]
        to: i64,
        /// Message ID to react to
        #[arg(long)]
        message_id: i64,
        /// Emoji to react with (e.g. "👍", "✅", "❤")
        emoji: String,
    },
    /// Migrate active bridge poller from this node to another mesh peer.
    Migrate {
        /// Target peer instance_name (as shown in `codeterm mesh peers`).
        #[arg(long)]
        to: String,
        /// Service to migrate (currently only "telegram").
        #[arg(long, default_value = "telegram")]
        service: String,
        /// Send a one-shot "now serving from <host>" message to the sentinel chat.
        #[arg(long)]
        announce: bool,
        /// Sentinel chat id for --announce. Defaults to the maintainer's chat.
        #[arg(long, default_value_t = 186809246)]
        announce_chat_id: i64,
        /// Skip auto-rollback if target claim fails (leave source disabled).
        #[arg(long)]
        no_rollback: bool,
        /// Skip preflight target-state check (for force migrations).
        #[arg(long)]
        force: bool,
        /// How long target may take to claim the Telegram lock before the migration is considered failed (ms).
        #[arg(long, default_value_t = 10_000)]
        claim_timeout_ms: u64,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Get a config value (dot-notation, e.g. "integrations.telegram_enabled")
    Get { key: String },
    /// Set a config value (JSON-compatible: true, false, 42, "string")
    Set { key: String, value: String },
}

#[derive(clap::Subcommand)]
pub enum WebAction {
    /// Print a ready-to-open /web URL for THIS machine with the API token
    /// embedded in the URL fragment (use --query for ?token= instead).
    Link {
        /// Host override (default: Tailscale IP > LAN IP > 127.0.0.1).
        #[arg(long)]
        host: Option<String>,
        /// Port override (default: this daemon's API port).
        #[arg(long)]
        port: Option<u16>,
        /// Emit ?token= instead of #token= — easier to type, but query
        /// strings reach server/proxy logs; the fragment never does.
        #[arg(long)]
        query: bool,
    },
}
