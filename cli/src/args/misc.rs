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
    /// List all panes
    List,
    /// Create a plain shell pane (no agent attached) in a workspace
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
}

#[derive(Subcommand)]
pub enum BrowserAction {
    /// Open a URL in the canvas panel
    Open {
        /// URL to open
        url: String,
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
