use clap::Subcommand;

#[derive(Subcommand)]
pub enum WorkspaceAction {
    /// Show a compact human-readable workspace → Team/Plan → Tab overview
    List,
    /// Print this tab's workspace ID. Resolution order: --workspace, then
    /// $CODETERM_TAB_ID, then $CODETERM_WORKSPACE_ID, then the active workspace.
    /// Exits 1 with empty stdout when no identity resolves.
    Current {
        /// Workspace id to report, validated against the daemon's workspace list
        #[arg(long, value_name = "WORKSPACE_ID")]
        workspace: Option<String>,
    },
    /// Create a new workspace. With --cwd, reuses an existing workspace at that cwd.
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
pub enum TabAction {
    /// List compact tab summaries in the current workspace. Use
    /// `tab inspect` for full tab details.
    ///
    /// Each tab carries a `backing.kind`: `pty` tabs (terminal agents,
    /// shells) have a real shell — `send` injects text you can execute.
    /// `chat_backend` tabs (LLM chat via plugin) have NO shell: `send`
    /// delivers a chat message; never instruct them to run shell commands.
    List {
        /// Workspace id to list. Defaults to this tab's workspace, then the focused workspace.
        #[arg(long, value_name = "WORKSPACE_ID")]
        workspace: Option<String>,
        /// Include every workspace instead of the current one.
        #[arg(long, conflicts_with = "workspace")]
        all_workspaces: bool,
        /// Show only tabs running an agent (have an agent_type).
        #[arg(long)]
        agents: bool,
    },
    /// Show the full daemon tab payload for one or more explicit tab ids.
    Inspect {
        /// Temporary positional aliases for tab ids. Prefer --tab.
        #[arg(num_args = 0..)]
        tabs: Vec<String>,
        /// One or more tab ids to inspect. Repeat the flag for multiple tabs.
        #[arg(long = "tab", value_name = "ID", action = clap::ArgAction::Append)]
        tab: Vec<String>,
        /// Include this tab's lifecycle journal, checkpoint, and recovery verdict.
        #[arg(long)]
        lifecycle: bool,
    },
    /// Plain shell tab. For an AI tab use `codeterm agent spawn` instead.
    New {
        /// Target workspace id (default: current focused)
        #[arg(long)]
        workspace: Option<String>,
        /// Working directory for the new tab
        #[arg(long)]
        cwd: Option<String>,
        /// Tab title
        #[arg(long)]
        title: Option<String>,
        /// Attach this shell to a Team as a utility terminal. The calling Tab
        /// must already belong to that Team.
        #[arg(long, value_name = "TEAM_ID")]
        team: Option<String>,
        /// Spawn a shell that runs this command on start (e.g.
        /// `"cd /repo && ./build.sh dev"`). Delivery waits for the PTY to be
        /// ready, so the command runs reliably even on a freshly-created tab.
        #[arg(long, allow_hyphen_values = true)]
        cmd: Option<String>,
        /// Spawn the shell PTY immediately on the daemon for headless/API use.
        #[arg(long)]
        eager: bool,
        /// Open a non-PTY chat-backend tab driven by the given plugin id
        /// (a plugin declaring the `chatBackend` capability).
        /// `codeterm send`/`tab get-text` then route to the plugin, not a shell.
        #[arg(long, value_name = "PLUGIN_ID")]
        chat_backend: Option<String>,
        /// JSON config object passed to the chat-backend plugin's `openSession`
        /// (only with `--chat-backend`). E.g. `'{"model":"llama-3.1-8b"}'`.
        #[arg(long, value_name = "JSON")]
        config: Option<String>,
    },
    /// Print the current focused tab ID — no JSON parsing needed
    Current {
        /// Return the last tab instead of the focused one
        #[arg(long)]
        last: bool,
    },
    /// Focus a tab by ID
    Focus {
        /// Temporary positional alias for the tab id. Prefer --tab.
        #[arg(conflicts_with = "tab")]
        id: Option<String>,
        /// Tab ID to focus.
        #[arg(long = "tab", value_name = "ID")]
        tab: Option<String>,
    },
    /// Replace a plain shell's process in the same tab, preserving tab identity and metadata.
    Restart {
        /// Tab ID to restart (default: current tab).
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Working directory for the replacement shell.
        #[arg(long)]
        cwd: Option<String>,
        /// Command to execute after the replacement shell is ready.
        #[arg(long, allow_hyphen_values = true)]
        cmd: Option<String>,
    },
    /// Close a tab by ID
    Close {
        /// Temporary positional alias for the tab id. Prefer --tab.
        #[arg(conflicts_with = "tab")]
        id: Option<String>,
        /// Tab ID to close.
        #[arg(long = "tab", value_name = "ID")]
        tab: Option<String>,
    },
    /// Rename a tab (default: current tab)
    Rename {
        /// New title
        title: String,
        /// Tab ID to rename (default: current focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Set a short tab description (empty clears it)
    Describe {
        /// Short summary, max 200 chars
        summary: String,
        /// Tab ID to describe (default: current focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Get visible terminal text of a tab (what the user sees)
    #[command(alias = "get_text", alias = "screen")]
    GetText {
        /// Tab ID (default: current focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Include raw ANSI escape codes (colors, cursor state) instead of plain text
        #[arg(long)]
        formatted: bool,
    },
    /// Execute a command in a tab and return output
    Exec {
        /// Command to execute
        command: String,
        /// Tab ID (default: current focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Timeout in milliseconds
        #[arg(long, default_value = "10000")]
        timeout: u64,
    },
    /// Show agent status for a tab (type, provider session, prompt)
    Status {
        /// Tab ID (default: current focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Fuzzy search tabs by id-prefix, tab title, friendly session name,
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
        /// Tab ID (default: current focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },

    /// Read a tab's parsed chat transcript (text/thinking/tool calls/results)
    Chat {
        /// Target tab ID (default: $CODETERM_TAB_ID or focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Only return messages from this index onward
        #[arg(long, default_value = "0")]
        since: usize,
    },

    /// Dev-only: tee a tab's raw PTY output into an asciicast v2 `.cast` file.
    Capture {
        #[command(subcommand)]
        action: TabCaptureAction,
    },
}

pub fn normalize_required_tab_selector(action: &mut TabAction) -> Result<(), String> {
    let (command, id, tab) = match action {
        TabAction::Focus { id, tab } => ("focus", id, tab),
        TabAction::Close { id, tab } => ("close", id, tab),
        _ => return Ok(()),
    };
    let selector = tab
        .take()
        .or_else(|| id.take())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("tab {command} requires --tab <ID>"))?;
    *id = Some(selector);
    Ok(())
}

pub fn required_tab_selector<'a>(
    id: &'a Option<String>,
    tab: &'a Option<String>,
    command: &str,
) -> Result<&'a str, String> {
    tab.as_deref()
        .or(id.as_deref())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("tab {command} requires --tab <ID>"))
}

/// Dev-only PTY capture tap (asciicast v2). Used to build agent-state fixtures.
#[derive(Subcommand)]
pub enum TabCaptureAction {
    /// Arm capture: tee raw PTY output (ANSI intact) + timestamps to `--out`.
    Start {
        /// Target tab ID (default: $CODETERM_TAB_ID or focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Path of the `.cast` file to write.
        #[arg(long)]
        out: String,
    },
    /// Disarm capture: flush/close the `.cast` and write a sibling `screen.txt`.
    Stop {
        /// Target tab ID (default: $CODETERM_TAB_ID or focused tab)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
}

/// Cross-mesh ephemeral pastebin (#98). In-memory; daemon restart wipes
/// every paste. 20-entry cap, oldest-first eviction. Use for handing
/// payloads between machines: stash a blob with `paste put --mesh PEER`,
/// reference the slug in the next CLI call, let the peer `paste get --pop` it.
#[derive(Subcommand)]
pub enum PasteAction {
    /// Stash a payload. Body from positional / --from-file / stdin.
    Put {
        /// Body (positional). Omit and use --from-file or --from-stdin.
        #[arg(allow_hyphen_values = true)]
        body: Option<String>,
        /// Read body from a file instead.
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,
        /// Read body from a relative file in the current agent tab's pad.
        #[arg(long, value_name = "PATH", conflicts_with_all = ["body", "from_file", "from_stdin"])]
        from_pad: Option<String>,
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
    /// List local (or --mesh <peer>) pastes — id, size, ttl_remaining.
    List,
    /// Explicit delete.
    Rm {
        /// Paste id.
        id: String,
    },
}

#[derive(Subcommand)]
pub enum NotifyAction {
    /// Send a notification
    Send {
        /// Notification title
        title: String,

        /// Notification body
        #[arg(long)]
        body: Option<String>,

        /// Notification level: info, warning, error
        #[arg(long, default_value = "info")]
        level: String,
    },
    /// List notifications
    List {
        /// Show only unread notifications
        #[arg(long)]
        unread: bool,
    },
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
        /// On Linux/GNOME, explicitly apply current-user power policy overrides
        /// (disable inactive sleep, idle blanking, lid-close and power-button suspend).
        #[arg(long)]
        gnome_policy: bool,
        /// Print system-level sudo mask commands in the status response.
        /// CodeTerm never runs these commands silently.
        #[arg(long)]
        system_block: bool,
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
        #[arg(allow_hyphen_values = true)]
        text: Option<String>,
        /// Read the text body from the current agent tab's pad
        #[arg(long, value_name = "PATH", conflicts_with = "text")]
        from_pad: Option<String>,
        /// Preserve literal escape sequences instead of decoding \n into a newline
        #[arg(long)]
        raw_text: bool,
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
        /// Sentinel chat id for --announce. Defaults to the target's first allow-listed bridge user.
        #[arg(long)]
        announce_chat_id: Option<i64>,
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
    Get { key: Option<String> },
    /// Set a config value (JSON-compatible: true, false, 42, "string")
    Set { key: String, value: String },
}

#[derive(clap::Subcommand)]
pub enum WebAction {
    /// Print a ready-to-open browser UI URL for THIS machine with the API token
    /// embedded in the URL fragment (use --query for ?token= instead).
    Link {
        /// Build a link for a paired mesh PEER (by instance name) instead of
        /// this machine: resolves the peer's host (prefers its HTTPS
        /// tailscale-serve FQDN) and mints the peer's web token. Overrides
        /// --host/--port.
        #[arg(long)]
        peer: Option<String>,
        /// Host override (default: Tailscale Serve HTTPS > Tailscale IP > LAN IP > 127.0.0.1).
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
