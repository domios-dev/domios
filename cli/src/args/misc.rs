use clap::builder::TypedValueParser;
use clap::Subcommand;
use codeterm_types::notification::NotifyLevel;

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
    /// Create a new workspace. With --cwd, reuses an existing workspace at that cwd. With --provision, the daemon creates a fresh directory for it.
    Create {
        /// Workspace name
        name: String,
        /// Working directory — agents spawned with --workspace inherit this as their CWD
        #[arg(long)]
        cwd: Option<String>,
        /// Create a new empty directory under the CodeTerm data dir as the cwd; agents started exactly there get directory trust
        #[arg(long, conflicts_with = "cwd")]
        provision: bool,
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
        /// Explicitly acknowledge destroying agent tabs in this workspace.
        #[arg(long)]
        force: bool,
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
        /// Include this tab's lifecycle journal telemetry: lineage, transitions, checkpoint.
        #[arg(long)]
        lifecycle: bool,
    },
    /// Plain shell tab, inheriting the calling tab's workspace and Team. For an
    /// AI tab use `codeterm agent spawn`.
    New {
        /// Target workspace id. Defaults to the calling tab's own workspace, then
        /// the focused workspace for a caller with no tab identity.
        #[arg(long)]
        workspace: Option<String>,
        /// Working directory for the new tab
        #[arg(long)]
        cwd: Option<String>,
        /// Tab title
        #[arg(long)]
        title: Option<String>,
        /// Explicit Team membership cross-check. Ordinary shells already inherit
        /// the calling Tab's Team.
        #[arg(long, value_name = "TEAM_ID")]
        team: Option<String>,
        /// Keep the new shell outside the calling tab's Team.
        #[arg(long, conflicts_with = "team")]
        no_team: bool,
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
        /// Tab ID to rename (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Set a short tab description (empty clears it)
    Describe {
        /// Short summary, max 200 chars
        summary: String,
        /// Tab ID to describe (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Get visible terminal text of a tab (what the user sees)
    #[command(alias = "get_text", alias = "screen")]
    GetText {
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
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
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Timeout in milliseconds
        #[arg(long, default_value = "10000")]
        timeout: u64,
    },
    /// Show agent status for a tab (type, provider session, prompt)
    Status {
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Answer the tab's current provider-declared prompt by numbered option.
    /// Sends only when a pending prompt exists; use this instead of raw keys.
    Respond {
        /// One-based option number shown by `tab status` or `tab inspect`.
        option: usize,
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Bind a running tab to an existing provider session without restarting it.
    Rebind {
        /// Provider session ID to bind.
        session_id: String,
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback).
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },
    /// Repair a live tab's provider identity from verified session evidence.
    Repair {
        /// Require the bound, nonce-verified session to belong to this provider.
        #[arg(long, value_name = "PROVIDER", conflicts_with_all = ["session", "from_evidence", "from_journal"])]
        provider: Option<String>,
        /// Rebind this verified session and reassert its provider identity.
        #[arg(long, value_name = "SESSION", conflicts_with_all = ["provider", "from_evidence", "from_journal"])]
        session: Option<String>,
        /// Derive provider and session from this tab's live evidence. This is the default.
        #[arg(long, conflicts_with_all = ["provider", "session", "from_journal"])]
        from_evidence: bool,
        /// Restore the newest eligible journal identity backed by verified session evidence.
        #[arg(long, conflicts_with_all = ["provider", "session", "from_evidence"])]
        from_journal: bool,
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback).
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
    /// provider's declared stopKey (Esc for all bundled providers). Plain
    /// shells get a 200 with {stopped:false} (no key). Designed for
    /// mesh/CLI/bot drivers — type `/stop` in a Telegram bridge and have it
    /// map straight here.
    Stop {
        /// Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
    },

    /// Read a tab's parsed chat transcript (text/thinking/tool calls/results)
    Chat {
        /// Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
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
        /// Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        /// Path of the `.cast` file to write.
        #[arg(long)]
        out: String,
    },
    /// Disarm capture: flush/close the `.cast` and write a sibling `screen.txt`.
    Stop {
        /// Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
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

/// Accepted values come straight from the daemon-shared set, so the CLI cannot
/// know a level the daemon rejects, and no triple is restated here.
fn notify_level_parser() -> impl clap::builder::TypedValueParser<Value = NotifyLevel> {
    clap::builder::PossibleValuesParser::new(NotifyLevel::wire_values()).map(|raw| {
        NotifyLevel::parse(&raw).expect("possible values are derived from NotifyLevel::ALL")
    })
}

#[derive(Subcommand)]
pub enum NotifyAction {
    /// Send a notification. The body is read only from the calling tab's AgentPad. Reports when no surface will show it.
    Send {
        /// Notification title
        title: String,

        /// Read the notification body from a relative file in the calling tab's pad
        #[arg(long, value_name = "PATH", conflicts_with_all = ["body", "from_file", "from_stdin"])]
        from_pad: Option<String>,

        // The refused sources stay declared so their use is a clap-level conflict with
        // the always-present title — a parse error cannot be reached past at runtime.
        /// REFUSED: an inline body is mangled by the shell — stage it in the pad
        #[arg(hide = true, conflicts_with_all = ["title", "from_pad"])]
        body: Option<String>,

        /// REFUSED: a file outside the pad has no provenance — stage it in the pad
        #[arg(long, value_name = "PATH", hide = true, conflicts_with_all = ["title", "from_pad"])]
        from_file: Option<String>,

        /// REFUSED: stdin is a shell pipe — stage the body in the pad
        #[arg(long, hide = true, conflicts_with_all = ["title", "from_pad"])]
        from_stdin: bool,

        /// Record identity axis: info, warning, error. Tone comes from --level
        #[arg(long, default_value = "info")]
        severity: String,

        /// What this notification is, on the one level axis. Omit to file it in the bell only.
        #[arg(long, value_parser = notify_level_parser())]
        level: Option<NotifyLevel>,

        /// Stable identity for coalescing. Derived from the title when omitted.
        #[arg(long)]
        event_key: Option<String>,

        /// Offer one tappable choice; repeat for each. Labels only — a
        /// notification takes a chosen label back, never typed text.
        #[arg(long = "choice", value_name = "LABEL")]
        choice: Vec<String>,

        /// Offer to turn one notification surface on: `desktop`, or a push
        /// channel's service id. The change waits for the tap — an agent may
        /// offer to be louder, never make itself louder.
        #[arg(long = "offer", value_name = "SURFACE", requires = "offer_accept")]
        offer: Option<String>,

        /// Which of this notification's own choices accepts the offer.
        #[arg(long = "offer-accept", value_name = "LABEL", requires = "offer")]
        offer_accept: Option<String>,

        /// The tab whose chat holds the message this notification is about
        #[arg(long = "target-tab", value_name = "TAB_ID")]
        target_tab: Option<String>,

        /// The exact chat message id inside --target-tab this notification jumps to
        #[arg(
            long = "chat-message",
            value_name = "MESSAGE_ID",
            requires = "target_tab"
        )]
        chat_message: Option<String>,
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::{Cli, Commands};

    use super::WorkspaceAction;

    #[test]
    fn workspace_create_parses_provision_without_cwd() {
        let parsed =
            Cli::try_parse_from(["codeterm", "workspace", "create", "bench", "--provision"])
                .expect("workspace create --provision parses");
        match parsed.command {
            Commands::Workspace {
                action:
                    WorkspaceAction::Create {
                        name,
                        cwd,
                        provision,
                    },
            } => {
                assert_eq!(name, "bench");
                assert!(cwd.is_none());
                assert!(provision);
            }
            _ => panic!("expected workspace create"),
        }
    }

    #[test]
    fn workspace_create_refuses_provision_with_cwd() {
        assert!(Cli::try_parse_from([
            "codeterm",
            "workspace",
            "create",
            "bench",
            "--provision",
            "--cwd",
            "/home/user/project",
        ])
        .is_err());
    }
}
