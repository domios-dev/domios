mod client;
mod mesh_cli;

use clap::{Parser, Subcommand};
use client::{discover_socket, SocketClient};

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC sequences: ESC [ ... final_byte
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc.is_ascii_alphabetic() || nc == '~' || nc == 'h' || nc == 'l' { break; }
                }
            } else if chars.peek() == Some(&']') {
                // OSC sequence: ESC ] ... BEL or ST
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc == '\x07' { break; }
                    if nc == '\x1b' { chars.next(); break; }
                }
            }
        } else if c == '\r' {
            // Skip carriage return
        } else {
            out.push(c);
        }
    }
    out
}
pub use mesh_cli::{MeshAction, MeshPeerAction};

/// Determine the CodeTerm data directory.
/// Dev instances use `~/.codeterm-dev/`, prod uses `~/.codeterm/`.
/// Detection priority:
///   1. CODETERM_SOCKET_PATH containing "codeterm-dev" (set by dev PTYs)
///   2. CODETERM_API_PORT == 7686 (dev's port — useful when calling CLI from
///      outside a dev pane, e.g. tests)
///   3. Default: prod
fn data_dir_name() -> &'static str {
    if let Ok(sp) = std::env::var("CODETERM_SOCKET_PATH") {
        if sp.contains("codeterm-dev") {
            return ".codeterm-dev";
        }
    }
    if std::env::var("CODETERM_API_PORT").as_deref() == Ok("7686") {
        return ".codeterm-dev";
    }
    ".codeterm"
}

/// Read API config (host, port, token).
/// Priority: $CODETERM_API_PORT env var (set by CodeTerm PTY) > config.toml.
/// This ensures CLI works in both dev (7686) and prod (7685) when called from inside CodeTerm.
pub fn read_api_config() -> (String, Option<String>) {
    let config_dir = dirs::home_dir().unwrap_or_default().join(data_dir_name());
    let config_path = config_dir.join("config.toml");

    let (mut host, mut port) = ("127.0.0.1".to_string(), 7685i64);

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(val) = content.parse::<toml::Table>() {
            if let Some(api) = val.get("api").and_then(|v| v.as_table()) {
                let raw_host = api.get("host").and_then(|v| v.as_str()).unwrap_or("127.0.0.1");
                host = if raw_host == "0.0.0.0" { "127.0.0.1".to_string() } else { raw_host.to_string() };
                port = api.get("port").and_then(|v| v.as_integer()).unwrap_or(7685);
            }
        }
    }

    // Override port from env — CodeTerm sets this in spawned PTYs
    if let Ok(env_port) = std::env::var("CODETERM_API_PORT") {
        if let Ok(p) = env_port.parse::<i64>() {
            port = p;
        }
    }

    // Try Ed25519 signed token first, fall back to simple API token from config
    let identity_path = config_dir.join("mesh").join("identity.json");
    let signed_token = std::fs::read_to_string(&identity_path).ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .and_then(|id| {
            let pk = id.get("private_key")?.as_str()?;
            let iid = id.get("instance_id")?.as_str()?;
            mesh_cli::sign_request(pk, iid).ok()
        });

    // Highest priority: explicit CODETERM_API_TOKEN env override.
    // Useful for hitting dev (port 7686) from outside its PTY without the
    // matching mesh identity, or for tests with a manually scoped token.
    let token = std::env::var("CODETERM_API_TOKEN").ok()
        .filter(|s| !s.is_empty())
        .or(signed_token)
        .or_else(|| {
            let config_path = config_dir.join("config.toml");
            std::fs::read_to_string(&config_path).ok()
                .and_then(|content| content.parse::<toml::Table>().ok())
                .and_then(|val| val.get("api")?.as_table()?.get("token")?.as_str().map(String::from))
        });

    (format!("http://{}:{}", host, port), token)
}

/// Build HTTP client with auth header
pub fn build_http(token: &Option<String>) -> reqwest::blocking::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(t) = token {
        if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t)) {
            headers.insert(reqwest::header::AUTHORIZATION, val);
        }
    }
    if let Ok(pane) = std::env::var("CODETERM_PANE_ID") {
        if let Ok(v) = reqwest::header::HeaderValue::from_str(&pane) {
            headers.insert("X-CodeTerm-Pane-Id", v);
        }
    }
    if let Ok(worker) = std::env::var("CODETERM_WORKER_ID") {
        if let Ok(v) = reqwest::header::HeaderValue::from_str(&worker) {
            headers.insert("X-CodeTerm-Worker-Id", v);
        }
    }
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

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
        /// Text to send
        text: String,

        /// Target pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,
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
}

#[derive(Subcommand)]
pub enum SessionsAction {
    /// Search past sessions by content across all registered providers.
    /// Results are tagged with `provider` so an agent can resume with the
    /// right launcher.
    Search {
        /// Query string. Terms are whitespace-split; every term must
        /// appear (case-insensitive) in at least one matching message.
        query: String,
        /// Filter by provider id (e.g. "claude"). Omit or pass "all" to
        /// search every registered provider.
        #[arg(long)]
        provider: Option<String>,
        /// Restrict to sessions tagged with this project path. Handy when
        /// you only want hits from the current repo.
        #[arg(long)]
        project: Option<String>,
        /// Max sessions returned.
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Max snippets shown per session.
        #[arg(long, default_value = "3")]
        max_snippets: usize,
    },
    /// Resume a past session in its original cwd. Atomic replacement for
    /// the "workspace create → agent spawn --args=--resume=UUID" dance
    /// which keeps failing because Claude/Codex look for the JSONL in the
    /// current cwd. This command: finds the session across providers,
    /// creates a workspace at the right project path, spawns an agent
    /// pane there, and hands the resume id to the provider correctly.
    Resume {
        /// Session id to resume (UUID).
        session_id: String,
        /// Tab title for the new pane. Defaults to "resumed <shortid>".
        #[arg(long)]
        title: Option<String>,
        /// Reuse an existing workspace instead of creating a new one.
        #[arg(long)]
        workspace_id: Option<String>,
        /// Custom name for the new workspace (default: basename of cwd).
        #[arg(long)]
        workspace_name: Option<String>,
    },
    /// Print metadata for a past session (provider, cwd, file path, times).
    /// Read-only probe — use before `sessions resume` to verify the session
    /// is still on disk.
    Get {
        session_id: String,
    },
}

#[derive(Subcommand)]
pub enum MemAction {
    /// Search across mem_items (FTS5 + hybrid scoring)
    Search {
        /// Query text
        query: String,
        /// Filter by kind: skill | fact | note | conversation | script | peer | preference | daily | learning | rule | summary
        #[arg(long)]
        kind: Option<String>,
        /// Filter by area tag
        #[arg(long)]
        area: Option<String>,
        /// Restrict to global / workspace / workspace:<id>
        #[arg(long)]
        scope: Option<String>,
        /// Max results (default 5)
        #[arg(long, default_value = "5")]
        limit: usize,
        /// Run LLM rerank on top candidates (requires ANTHROPIC_API_KEY)
        #[arg(long)]
        rerank: bool,
    },
    /// Print one item by id
    Get {
        /// Mem item id (e.g. skill_home_assistant)
        id: String,
    },
    /// Delete an item by id
    Delete {
        id: String,
    },
    /// Print the full audit trail for one item (create/update/promote/delete).
    History {
        /// Mem item id
        id: String,
    },
    /// Print {area -> {kind -> count}} for the store (or a single workspace).
    Taxonomy {
        /// Restrict to one workspace scope
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Show recent mem writes (newest first). With --follow, poll for new rows.
    Tail {
        /// Follow mode: poll every 500ms and print new rows as they appear.
        #[arg(long, short = 'f')]
        follow: bool,
        /// Max rows per fetch (default 20)
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Partial update of an existing mem item by id. Only the flags you
    /// pass are written; everything else stays as-is.
    Update {
        /// Mem item id
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        area: Option<String>,
        /// Comma-separated tags (REPLACES existing tags)
        #[arg(long)]
        tags: Option<String>,
        /// New body content (re-indexes embedding)
        #[arg(long)]
        body: Option<String>,
        /// New importance 1-5
        #[arg(long)]
        importance: Option<f64>,
    },
    /// Save a new mem item
    Save {
        /// Item kind: skill | fact | note | conversation | script | peer | preference | daily | learning | rule | summary
        #[arg(long)]
        kind: String,
        /// Title (will be slugified into id). If omitted, first line of body.
        #[arg(long)]
        title: Option<String>,
        /// Body content. If omitted and stdin is piped, read stdin.
        body: Option<String>,
        /// Optional summary (~50-200 chars)
        #[arg(long)]
        summary: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// Area tag (auth | deploy | home | ...)
        #[arg(long)]
        area: Option<String>,
        /// Importance 1-5 (default 3.0). Higher = more likely in L1 wake-up.
        #[arg(long, default_value = "3.0")]
        importance: f64,
        /// Scope: global (default) or workspace
        #[arg(long)]
        scope: Option<String>,
        /// Path to source file on disk. Enables Librarian auto-sync.
        #[arg(long)]
        source_path: Option<String>,
        /// Overwrite an existing row with the same id (upsert). Use this to
        /// update the bundled identity placeholder or any pinned record.
        #[arg(long)]
        replace: bool,
        /// Skip auto-extraction worker for this save.
        #[arg(long)]
        skip_extract: bool,
    },
    /// Print L0+L1 wake-up context (GA init injection)
    WakeUp {
        /// Total token budget (split L0 + L1)
        #[arg(long)]
        budget: Option<usize>,
        /// Identity layer only (~100 tokens)
        #[arg(long)]
        l0_only: bool,
        /// Skip identity layer (return L1 only)
        #[arg(long)]
        no_l0: bool,
    },
    /// Re-run extraction workers for items that haven't been extracted yet.
    /// Useful after LLM API failures, quota resets, or bulk imports with
    /// skip_extract. Workers fire sequentially with a short delay.
    #[clap(name = "extract-pending")]
    ExtractPending {
        /// Maximum number of items to queue (default 200).
        #[arg(long, default_value = "200")]
        limit: usize,
    },
    /// Skill management — folder-on-disk skills with mem index.
    /// Skills live as ~/.codeterm/skills/<name>/SKILL.md (+ optional support files).
    /// `add` registers the folder; Librarian re-vectorizes if SKILL.md changes.
    Skill {
        #[command(subcommand)]
        action: MemSkillAction,
    },
    /// Show mem store stats (counts by kind, blob sizes)
    Stats,
    /// Capture a quick learning (insight, error, feature request)
    Learn {
        /// What you learned. If omitted and stdin piped, read stdin.
        text: Option<String>,
        /// "insight" (default), "error", or "feature"
        #[arg(long)]
        kind: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// Shortcut: --error <text> = --kind error
        #[arg(long)]
        error: Option<String>,
        /// Shortcut: --feature <text> = --kind feature
        #[arg(long)]
        feature: Option<String>,
    },
    /// List pending learnings (kind=learning items)
    LearnList,
    /// Promote a learning to a fact (raises importance)
    LearnPromote {
        id: String,
    },
    /// Flush a pane's PTY scrollback as a kind=conversation mem_item
    /// (called by PreCompact hook before context summarization).
    Flush {
        /// Pane id (default: $CODETERM_PANE_ID)
        #[arg(long)]
        pane: Option<String>,
        /// Optional title hint
        #[arg(long)]
        title: Option<String>,
    },
    /// Run dreaming consolidation (light + deep phases)
    Dream,
    /// Trigger file sync for source-tracked items (Librarian)
    Sync {
        /// Sync one file by path
        #[arg(long)]
        path: Option<String>,
    },
    /// Graph operations
    Graph {
        #[command(subcommand)]
        action: MemGraphAction,
    },
    /// Graph-worker operations (hidden mem panel agents)
    Worker {
        #[command(subcommand)]
        action: MemWorkerAction,
    },
    /// Run a single mem maintenance pass in this subprocess. Opens the DB
    /// directly (multi-process WAL) and performs one phase of work, then
    /// exits. Used by the daemon's supervisor so a panic / OOM in dreaming
    /// or librarian cannot take the API down.
    #[clap(name = "run-worker")]
    RunWorker {
        /// Which phase to run: dreaming-light | dreaming-deep | librarian-once
        kind: String,
        /// Override the path to knowledge.db. Defaults to the same data dir
        /// the CLI uses elsewhere (`~/.codeterm/mem/knowledge.db` for prod,
        /// `~/.codeterm-dev/mem/knowledge.db` for dev).
        #[arg(long)]
        db_path: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum MemWorkerAction {
    /// Spawn a new graph worker
    Spawn {
        #[arg(long)]
        intent: String,
        #[arg(long)]
        task: String,
        #[arg(long, default_value = "claude")]
        agent: String,
        #[arg(long)]
        parent: Option<String>,
    },
    /// List all graph workers
    List,
    /// Get one worker by id
    Get { id: String },
    /// Report progress (called by worker itself; uses $CODETERM_WORKER_ID)
    Progress {
        text: String,
        #[arg(long)]
        status: Option<String>,
    },
    /// Submit final report and mark done (uses $CODETERM_WORKER_ID)
    Report { summary: String },
    /// Kill a worker by id
    Kill { id: String },
}

#[derive(Subcommand)]
pub enum MemSkillAction {
    /// Scaffold a new skill folder under ~/.codeterm/skills/<name>/, write
    /// a SKILL.md template (with frontmatter + canonical sections) plus an
    /// empty scripts/ subdir, then register it in mem immediately. After
    /// this, edit the SKILL.md — Librarian will re-index on change.
    New {
        /// Skill slug (also folder name). Use kebab-case.
        name: String,
        /// Optional area tag to put in the frontmatter.
        #[arg(long)]
        area: Option<String>,
    },
    /// Register an existing skill folder. Reads <folder>/SKILL.md and saves
    /// a mem item with source_path linked to it. Re-runs are idempotent.
    /// Area can come from --area flag, YAML frontmatter (`area: foo`), or
    /// the parent folder name (~/.codeterm/skills/<area>/<skill>/SKILL.md).
    Add {
        /// Folder containing SKILL.md (e.g. ~/.codeterm/skills/mem-testing).
        folder: String,
        /// Override the auto-detected area.
        #[arg(long)]
        area: Option<String>,
        /// Importance 1-5, default 4.
        #[arg(long, default_value = "4")]
        importance: f64,
    },
    /// List all registered skills.
    List,
}

#[derive(Subcommand)]
pub enum MemGraphAction {
    /// Create a graph entity
    Create {
        /// Entity id (e.g. person_kai)
        #[arg(long)]
        id: String,
        /// Entity type (Person, Project, Task, Skill, ...)
        #[arg(long, name = "type")]
        entity_type: String,
        /// JSON properties
        #[arg(long, default_value = "{}")]
        props: String,
        #[arg(long)]
        scope: Option<String>,
    },
    /// Create a relation between entities or mem_items
    Relate {
        #[arg(long)]
        from: String,
        #[arg(long)]
        rel: String,
        #[arg(long)]
        to: String,
        /// Validity start (ISO 8601, default now)
        #[arg(long)]
        valid_from: Option<String>,
        /// Validity end (ISO 8601, optional)
        #[arg(long)]
        valid_to: Option<String>,
        #[arg(long, default_value = "1.0")]
        confidence: f64,
        #[arg(long)]
        scope: Option<String>,
    },
    /// List outgoing relations from an id
    Related {
        #[arg(long)]
        id: String,
        /// Filter by relation type
        #[arg(long)]
        rel: Option<String>,
        /// As-of date (ISO 8601) — filters by valid_from/valid_to
        #[arg(long)]
        as_of: Option<String>,
    },
    /// Invalidate a relation (set valid_to, preserve history)
    Invalidate {
        #[arg(long)]
        from: String,
        #[arg(long)]
        rel: String,
        #[arg(long)]
        to: String,
        /// When it ended (ISO 8601, default now)
        #[arg(long)]
        ended: Option<String>,
    },
    /// Full temporal timeline for an entity
    Timeline {
        #[arg(long)]
        entity: String,
    },
    /// BFS traversal from an entity (max_hops cap)
    Traverse {
        #[arg(long)]
        from: String,
        #[arg(long, default_value = "2")]
        hops: usize,
        #[arg(long, default_value = "100")]
        max_nodes: usize,
        #[arg(long)]
        as_of: Option<String>,
    },
    /// Shortest path between two entities
    Path {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, default_value = "4")]
        max_hops: usize,
    },
    /// List entities bridging 2+ areas (cross-project connections)
    Tunnels,
    /// Merge one entity into another (alias → canonical)
    Alias {
        #[arg(long)]
        alias: String,
        #[arg(long)]
        canonical: String,
    },
}

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
pub enum AgentAction {
    /// List all agent panes (with roles)
    List,
    /// Show full hierarchy tree
    Hierarchy,
    /// Mark a pane as orchestrator (top-level, can spawn leads/workers)
    #[command(alias = "set-orchestrator")]
    SetConductor {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Link a pane as worker under a parent
    Link {
        /// Parent pane ID (orchestrator or lead)
        #[arg(long)]
        parent: String,
        /// Pane ID to link (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Remove a pane's role (also orphans children)
    Unlink {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// List children of a pane
    Children {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Toggle whether a pane can spawn children
    AllowSpawn {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
        /// Enable or disable spawning
        #[arg(long, default_value = "true")]
        enable: bool,
    },
    /// Send a report to your parent in the hierarchy
    Report {
        /// Report message (also accepts --message alias)
        message: String,
        /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Read reports (for orchestrator panes)
    Reports {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
        /// Clear reports after reading
        #[arg(long)]
        clear: bool,
        /// Show only the N most recent reports (default: all)
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Get detailed status of a pane
    Status {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Compact agent context (sends /compact to Claude)
    Compact {
        /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Restart agent (gracefully /exit + auto-resume to pick up settings or recover)
    Restart {
        /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long)]
        pane: Option<String>,
        /// Skip the graceful exit and force-kill instead. Use only when the agent is hung.
        #[arg(long)]
        force: bool,
        /// Wait this long for graceful exit before force-killing (seconds)
        #[arg(long, default_value = "10")]
        grace_secs: u64,
    },
    /// Switch agent model (sends /model to Claude)
    Model {
        /// Model name: sonnet, haiku, opus
        name: String,
        /// Pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long)]
        pane: Option<String>,
    },
    /// List available agent providers (claude, codex, ...) with their
    /// display names, default models, and capability flags.
    Providers,
    /// List available models for a specific provider.
    Models {
        /// Provider id (e.g. "claude"). Required.
        provider: String,
    },
    /// Spawn a new agent (creates tab in workspace, linked in hierarchy)
    Spawn {
        /// Agent to run: claude, aider, goose, or any command
        #[arg(default_value = "claude")]
        agent: String,
        /// Tab title
        #[arg(long)]
        title: Option<String>,
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Role: Orchestrator, Lead, Worker
        #[arg(long)]
        role: Option<String>,
        /// Parent pane ID (for Worker/SubWorker)
        #[arg(long)]
        parent: Option<String>,
        /// Allow spawning children
        #[arg(long)]
        can_spawn: bool,
        /// Task to send after agent starts
        #[arg(long)]
        task: Option<String>,
        /// Extra args for the agent
        #[arg(long, num_args = 1..)]
        args: Option<Vec<String>>,
        /// Target workspace ID (default: parent's workspace or active workspace)
        #[arg(long)]
        workspace: Option<String>,
    },
}

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
    },
    /// Get or set the update channel (stable, rc, nightly)
    Channel {
        /// New channel value. Omit to print current channel.
        channel: Option<String>,
    },
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
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Get a config value (dot-notation, e.g. "integrations.telegram_enabled")
    Get { key: String },
    /// Set a config value (JSON-compatible: true, false, 42, "string")
    Set { key: String, value: String },
}

#[derive(Subcommand)]
pub enum CronAction {
    /// List cron jobs
    List {
        /// Filter by workspace ID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Create a cron job
    Create {
        /// Cron job name
        name: String,
        /// Cron schedule (5-field: "*/30 * * * *") or oneshot time ("in 5m", ISO8601)
        #[arg(long)]
        schedule: String,
        /// Task prompt for the agent
        #[arg(long)]
        task: String,
        /// Workspace ID (default: active)
        #[arg(long)]
        workspace: Option<String>,
        /// Agent to spawn (default: claude)
        #[arg(long, default_value = "claude")]
        agent: String,
        /// Model within the agent provider (e.g. "opus", "haiku",
        /// "gpt-5.4-mini"). Leave unset to use the provider default.
        #[arg(long)]
        model: Option<String>,
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Pane ID to receive reports from cron agent (default: $CODETERM_PANE_ID)
        #[arg(long, env = "CODETERM_PANE_ID")]
        creator_pane: Option<String>,
        /// Oneshot: fire once at scheduled time, then auto-delete
        #[arg(long)]
        once: bool,
    },
    /// Edit a cron job
    Edit {
        /// Cron job ID
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        schedule: Option<String>,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Agent provider override (e.g. "claude", "codex")
        #[arg(long)]
        agent: Option<String>,
        /// Model within the agent provider. Pass empty string to clear.
        #[arg(long)]
        model: Option<String>,
        /// Max jitter in milliseconds added to each scheduled fire. Set
        /// 0 to disable jitter. Omit to leave unchanged.
        #[arg(long)]
        jitter_max_ms: Option<u64>,
    },
    /// Delete a cron job
    Delete {
        /// Cron job ID
        id: String,
    },
    /// Enable a cron job
    Enable {
        /// Cron job ID
        id: String,
    },
    /// Disable a cron job
    Disable {
        /// Cron job ID
        id: String,
    },
    /// Stop any currently-running executions of a cron job without
    /// deleting the job definition. Future scheduled fires still run.
    Cancel {
        /// Cron job ID
        id: String,
    },
    /// Manually trigger a cron job now
    Trigger {
        /// Cron job ID
        id: String,
    },
    /// Show run history
    History {
        /// Cron job ID
        id: String,
    },
    /// Mark a cron run as complete (called by agent at end of task)
    Complete {
        /// Run ID
        #[arg(long)]
        run_id: String,
        /// Summary of what happened
        #[arg(long)]
        summary: Option<String>,
    },
}

/// Resolve pane ID: explicit arg > $CODETERM_PANE_ID env var > focused pane from server.
///
/// When running inside CodeTerm, $CODETERM_PANE_ID is set automatically by the PTY.
/// This ensures commands like `pane split` always target the caller's pane,
/// not whatever pane the user happens to have focused.
fn resolve_pane(client: &mut SocketClient, id: Option<String>) -> String {
    // 1. Explicit --pane/--id arg
    if let Some(id) = id {
        return id;
    }
    // 2. $CODETERM_PANE_ID env var (set by CodeTerm PTY for each shell)
    if let Ok(env_id) = std::env::var("CODETERM_PANE_ID") {
        if !env_id.is_empty() {
            return env_id;
        }
    }
    // 3. Fallback: ask server for focused pane
    match client.call("pane.list", None) {
        Ok(value) => {
            if let Some(arr) = value.as_array() {
                let pane = arr.iter()
                    .find(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                    .or_else(|| arr.first());
                if let Some(id) = pane.and_then(|p| p.get("id").and_then(|v| v.as_str())) {
                    return id.to_string();
                }
            }
            eprintln!("Error: no panes found");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn main() {
    let cli = Cli::parse();

    // --mesh TARGET: proxy entire command to a remote peer via relay
    if let Some(ref mesh_target) = cli.mesh {
        mesh_cli::run_via_mesh(&cli, mesh_target);
        return;
    }

    // Config command uses HTTP API directly — no socket needed
    let json_output = cli.json;
    if let Commands::Config { ref action } = cli.command {
        let (base_url, token) = read_api_config();
        let http = build_http(&token);
        let result = match action {
            ConfigAction::Get { key } => {
                let url = format!("{}/api/config?key={}", base_url, key);
                match http.get(&url).send() {
                    Ok(resp) if resp.status().is_success() => resp.json().map_err(|e| e.to_string()),
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().unwrap_or_default();
                        if status.as_u16() == 404 {
                            Err(format!("Key not found: {}", key))
                        } else {
                            Err(format!("API error ({}): {}", status, body))
                        }
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            ConfigAction::Set { key, value } => {
                let json_value: serde_json::Value = serde_json::from_str(value)
                    .unwrap_or_else(|_| serde_json::Value::String(value.clone()));
                let body = serde_json::json!({ "key": key, "value": json_value });
                match http.patch(&format!("{}/api/config", base_url)).json(&body).send() {
                    Ok(resp) if resp.status().is_success() => {
                        Ok(serde_json::json!({"ok": true, "key": key, "value": json_value}))
                    }
                    Ok(resp) => Err(format!("Failed: {}", resp.text().unwrap_or_default())),
                    Err(e) => Err(e.to_string()),
                }
            }
        };
        match result {
            Ok(value) => {
                if json_output {
                    println!("{}", serde_json::to_string_pretty(&value).unwrap());
                } else if let Some(v) = value.get("value") {
                    // For config get: print just the value
                    match v {
                        serde_json::Value::String(s) => println!("{}", s),
                        serde_json::Value::Null => println!("null"),
                        other => println!("{}", other),
                    }
                } else {
                    print_human(&value);
                }
            }
            Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
        }
        return;
    }

    let socket_path = match discover_socket() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut client = match SocketClient::connect(&socket_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let result = match cli.command {
        Commands::Identify => client.call("system.identify", None),

        Commands::Capabilities => client.call("system.capabilities", None),

        Commands::Workspace { action } => match action {
            WorkspaceAction::List => client.call("workspace.list", None),
            WorkspaceAction::Current => {
                let result = client.call("workspace.list", None);
                match result {
                    Ok(value) => {
                        if let Some(arr) = value.as_array() {
                            let id = arr.iter()
                                .find(|w| w.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                                .or_else(|| arr.first())
                                .and_then(|w| w.get("id").and_then(|v| v.as_str()));
                            if let Some(id) = id {
                                println!("{}", id);
                            }
                        }
                        return;
                    }
                    Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                }
            }
            WorkspaceAction::Create { name, cwd } => {
                let mut params = serde_json::json!({ "name": name });
                if let Some(cwd) = cwd {
                    params["cwd"] = serde_json::json!(cwd);
                }
                client.call("workspace.create", Some(params))
            }
            WorkspaceAction::Focus { id } => {
                client.call("workspace.focus", Some(serde_json::json!({ "workspace_id": id })))
            }
            WorkspaceAction::Rename { id, name } => {
                client.call("workspace.rename", Some(serde_json::json!({ "workspace_id": id, "name": name })))
            }
            WorkspaceAction::Close { id } => {
                client.call("workspace.close", Some(serde_json::json!({ "workspace_id": id })))
            }
            WorkspaceAction::MoveTab { tab_id, from, to } => {
                client.call("workspace.move_tab", Some(serde_json::json!({
                    "tab_id": tab_id,
                    "from_workspace_id": from,
                    "to_workspace_id": to
                })))
            }
        },

        Commands::Browser { action } => match action {
            BrowserAction::Open { url } => {
                client.call("browser.open", Some(serde_json::json!({ "url": url })))
            }
        },

        Commands::Pane { action } => match action {
            PaneAction::List => client.call("pane.list", None),
            PaneAction::Current { last } => {
                let result = client.call("pane.list", None);
                match result {
                    Ok(value) => {
                        if let Some(arr) = value.as_array() {
                            let pane = if last {
                                arr.last()
                            } else {
                                arr.iter()
                                    .find(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                                    .or_else(|| arr.first())
                            };
                            if let Some(id) = pane.and_then(|p| p.get("id").and_then(|v| v.as_str())) {
                                println!("{}", id);
                            }
                        }
                        return;
                    }
                    Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                }
            }
            PaneAction::Focus { id } => {
                client.call("pane.focus", Some(serde_json::json!({ "pane_id": id })))
            }
            PaneAction::Close { id } => {
                client.call("pane.close", Some(serde_json::json!({ "pane_id": id })))
            }
            PaneAction::Rename { pane, title } => {
                let pane_id = resolve_pane(&mut client, pane);
                client.call(
                    "pane.rename",
                    Some(serde_json::json!({ "pane_id": pane_id, "title": title })),
                )
            }
            PaneAction::GetText { pane, formatted } => {
                let pane_id = resolve_pane(&mut client, pane);
                let (api_url, api_token) = read_api_config();
                let http = build_http(&api_token);
                let url = format!("{}/api/panes/{}/text?formatted={}", api_url, pane_id, formatted);
                match http.get(url).send() {
                    Ok(resp) => {
                        if let Ok(val) = resp.json::<serde_json::Value>() {
                            if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
                                if formatted {
                                    print!("{}", text);
                                } else {
                                    for line in text.lines() {
                                        if !line.trim().is_empty() {
                                            println!("{}", line);
                                        }
                                    }
                                }
                                return;
                            }
                        }
                        eprintln!("No pane text");
                        std::process::exit(1);
                    }
                    Err(e) => { eprintln!("Get-text request failed: {}", e); std::process::exit(1); }
                }
            }
            PaneAction::Exec { pane, command, timeout } => {
                let pane_id = resolve_pane(&mut client, pane);
                let (api_url, api_token) = read_api_config();
                let http = build_http(&api_token);
                match http.post(format!("{}/api/panes/{}/exec", api_url, pane_id))
                    .json(&serde_json::json!({
                        "command": command,
                        "timeout_ms": timeout,
                        "strip_ansi": true,
                    }))
                    .send()
                {
                    Ok(resp) => {
                        if let Ok(val) = resp.json::<serde_json::Value>() {
                            if let Some(output) = val.get("output").and_then(|v| v.as_str()) {
                                print!("{}", output);
                                return;
                            }
                        }
                        eprintln!("Exec failed");
                        std::process::exit(1);
                    }
                    Err(e) => { eprintln!("Exec request failed: {}", e); std::process::exit(1); }
                }
            }
            PaneAction::Status { pane } => {
                let pane_id = resolve_pane(&mut client, pane);
                let (api_url, api_token) = read_api_config();
                let http = build_http(&api_token);
                match http.get(format!("{}/api/panes", api_url)).send() {
                    Ok(resp) => {
                        if let Ok(panes) = resp.json::<Vec<serde_json::Value>>() {
                            if let Some(p) = panes.iter().find(|p| p.get("id").and_then(|v| v.as_str()) == Some(&pane_id)) {
                                let at = p.get("agent_type").and_then(|v| v.as_str()).unwrap_or("-");
                                let sid = p.get("agent_session_id").and_then(|v| v.as_str()).unwrap_or("-");
                                let pp = if p.get("pending_prompt").is_some() { "yes" } else { "no" };
                                let cv = p.get("chat_view").and_then(|v| v.as_bool()).unwrap_or(false);
                                println!("pane:    {}", pane_id);
                                println!("agent:   {}", at);
                                println!("session: {}", sid);
                                println!("prompt:  {}", pp);
                                println!("chat:    {}", cv);
                                return;
                            }
                        }
                        eprintln!("Pane not found");
                        std::process::exit(1);
                    }
                    Err(e) => { eprintln!("Status request failed: {}", e); std::process::exit(1); }
                }
            }
        },

        Commands::GetOutput { pane, lines } => {
            let pane_id = resolve_pane(&mut client, pane);
            let max_bytes = (lines as u64) * 80;
            let result = client.call(
                "pane.get_output",
                Some(serde_json::json!({ "pane_id": pane_id, "max_bytes": max_bytes })),
            );
            match result {
                Ok(value) => {
                    if let Some(output) = value.get("output").and_then(|v| v.as_str()) {
                        if cli.json {
                            println!("{}", serde_json::to_string_pretty(&value).unwrap());
                        } else {
                            print!("{}", strip_ansi(output));
                        }
                    }
                    return;
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::SendText { pane, text } => {
            let pane_id = resolve_pane(&mut client, pane);
            client.call(
                "pane.send_text",
                Some(serde_json::json!({ "pane_id": pane_id, "text": text })),
            )
        }

        Commands::Notify { title, body, level } => {
            let mut params = serde_json::json!({ "title": title, "level": level });
            if let Some(b) = body {
                params["body"] = serde_json::Value::String(b);
            }
            client.call("notification.create", Some(params))
        }

        Commands::Bridge { action } => match action {
            BridgeAction::Restart => {
                client.call("bridge.restart", None)
            }
            BridgeAction::Send { text, service, to, append, r#type, file, caption, filename, buttons } => {
                let mut params = serde_json::json!({
                    "service": service,
                    "chat_id": to,
                    "type": r#type,
                    "append": append,
                });
                if let Some(t) = text {
                    params["text"] = serde_json::Value::String(t);
                }
                if let Some(f) = file {
                    params["file"] = serde_json::Value::String(f);
                }
                if let Some(c) = caption {
                    params["caption"] = serde_json::Value::String(c);
                }
                if let Some(n) = filename {
                    params["filename"] = serde_json::Value::String(n);
                }
                if let Some(b) = buttons {
                    // Parse buttons JSON
                    match serde_json::from_str::<serde_json::Value>(&b) {
                        Ok(v) => { params["buttons"] = v; }
                        Err(e) => {
                            eprintln!("Invalid buttons JSON: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                client.call("bridge.send", Some(params))
            }
            BridgeAction::React { to, message_id, emoji } => {
                client.call("bridge.react", Some(serde_json::json!({
                    "chat_id": to,
                    "message_id": message_id,
                    "emoji": emoji,
                })))
            }
        },

        Commands::Notifications { action, unread } => match action {
            Some(NotificationAction::Clear) => client.call("notification.clear", None),
            None => client.call(
                "notification.list",
                Some(serde_json::json!({ "unread_only": unread })),
            ),
        },

        Commands::Keepawake { action } => {
            let (base, token) = read_api_config();
            let http = build_http(&token);
            let result: Result<serde_json::Value, String> = match action.unwrap_or(KeepAwakeAction::Status) {
                KeepAwakeAction::Status => http
                    .get(format!("{}/api/keepawake", base))
                    .send()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.json::<serde_json::Value>().map_err(|e| e.to_string())),
                KeepAwakeAction::Start { duration_secs, display } => http
                    .post(format!("{}/api/keepawake/start", base))
                    .json(&serde_json::json!({
                        "duration_secs": duration_secs,
                        "display": display,
                    }))
                    .send()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.json::<serde_json::Value>().map_err(|e| e.to_string())),
                KeepAwakeAction::Stop => http
                    .post(format!("{}/api/keepawake/stop", base))
                    .send()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.json::<serde_json::Value>().map_err(|e| e.to_string())),
            };
            match result {
                Ok(val) => {
                    if cli.json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        print_object(&val);
                    }
                    return;
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Agent { action } => {
            let (base, token) = read_api_config();
            let http = build_http(&token);
            match action {
                AgentAction::List => {
                    match http.get(format!("{}/api/agents", base)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() { println!("No agents found."); }
                                for a in arr { print_object(a); println!(); }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Providers => {
                    match http.get(format!("{}/api/agents/providers", base)).send() {
                        Ok(resp) => {
                            let status = resp.status();
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                                return;
                            }
                            let Some(arr) = val.as_array() else {
                                if status == 404 {
                                    eprintln!(
                                        "Server doesn't know /api/agents/providers — update the running CodeTerm instance."
                                    );
                                } else {
                                    eprintln!("Unexpected response shape (HTTP {})", status);
                                }
                                std::process::exit(1);
                            };
                            if arr.is_empty() {
                                println!("No agent providers registered.");
                                return;
                            }
                            for p in arr {
                                let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                                let name = p.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
                                let default_model = p.get("default_model").and_then(|v| v.as_str()).unwrap_or("-");
                                let models_count = p
                                    .get("available_models")
                                    .and_then(|v| v.as_array())
                                    .map(|a| a.len())
                                    .unwrap_or(0);
                                println!("{:<12} {:<20} default={}  models={}", id, name, default_model, models_count);
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Models { provider } => {
                    match http.get(format!("{}/api/agents/providers", base)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            let Some(arr) = val.as_array() else {
                                eprintln!("Unexpected response shape");
                                std::process::exit(1);
                            };
                            let target = arr.iter().find(|p| {
                                p.get("id").and_then(|v| v.as_str()) == Some(provider.as_str())
                            });
                            let Some(target) = target else {
                                let known: Vec<&str> = arr.iter()
                                    .filter_map(|p| p.get("id").and_then(|v| v.as_str()))
                                    .collect();
                                eprintln!(
                                    "Unknown provider: '{}'. Known: [{}]",
                                    provider,
                                    known.join(", ")
                                );
                                std::process::exit(1);
                            };
                            let models = target.get("available_models").and_then(|v| v.as_array());
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&models).unwrap());
                                return;
                            }
                            let Some(models) = models else {
                                println!("No models advertised for provider '{}'.", provider);
                                return;
                            };
                            let default_model = target.get("default_model").and_then(|v| v.as_str());
                            for m in models {
                                let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                                let name = m.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
                                let marker = if default_model == Some(id) { " (default)" } else { "" };
                                let description = m.get("description").and_then(|v| v.as_str()).unwrap_or("");
                                if description.is_empty() {
                                    println!("{:<10} {}{}", id, name, marker);
                                } else {
                                    println!("{:<10} {}{}\n             {}", id, name, marker, description);
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Hierarchy => {
                    match http.get(format!("{}/api/agents/hierarchy", base)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                fn print_tree(node: &serde_json::Value, indent: usize) {
                                    let prefix = "  ".repeat(indent);
                                    let role = node.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                                    let title = node.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                                    let status = node.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                                    let icon = match role {
                                        "Orchestrator" => "★",
                                        "Lead" => "·",
                                        "Worker" => "⚙",
                                        _ => "·",
                                    };
                                    println!("{}{} {} [{}] ({})", prefix, icon, title, status, role);
                                    if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
                                        for c in children { print_tree(c, indent + 1); }
                                    }
                                }
                                if let Some(arr) = val.as_array() {
                                    if arr.is_empty() { println!("No hierarchy found."); }
                                    for c in arr { print_tree(c, 0); }
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::SetConductor { pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    match http.post(format!("{}/api/panes/{}/role", base, pane_id))
                        .json(&serde_json::json!({"role": "Orchestrator"}))
                        .send()
                    {
                        Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Link { parent, pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    // Determine role based on parent's role
                    let parent_resp = http.get(format!("{}/api/panes/{}/status", base, parent)).send();
                    let role = match parent_resp {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            match val.get("role").and_then(|v| v.as_str()) {
                                Some("Orchestrator") | Some("Conductor") => "Worker",
                                Some("Lead") | Some("Worker") => "Worker",
                                _ => { eprintln!("Parent must be Orchestrator or Lead"); std::process::exit(1); }
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    };
                    match http.post(format!("{}/api/panes/{}/role", base, pane_id))
                        .json(&serde_json::json!({"role": role, "parent_pane_id": parent}))
                        .send()
                    {
                        Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Unlink { pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    match http.delete(format!("{}/api/panes/{}/role", base, pane_id)).send() {
                        Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Children { pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    match http.get(format!("{}/api/panes/{}/children", base, pane_id)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() { println!("No children."); }
                                for c in arr { print_object(c); println!(); }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::AllowSpawn { pane, enable } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    match http.post(format!("{}/api/panes/{}/can-spawn", base, pane_id))
                        .json(&serde_json::json!({"can_spawn": enable}))
                        .send()
                    {
                        Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Report { message, pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    // Use socket (not REST) for reliability
                    client.call("agent.report", Some(serde_json::json!({
                        "pane_id": pane_id,
                        "message": message,
                    })))
                }
                AgentAction::Reports { pane, clear, limit } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    // Use socket (not REST) for reliability
                    match client.call("agent.reports", Some(serde_json::json!({
                        "pane_id": pane_id,
                        "clear": clear,
                    }))) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() { println!("No reports."); }
                                let take = limit.unwrap_or(arr.len());
                                for r in arr.iter().rev().take(take) {
                                    let from = r.get("from_title").and_then(|v| v.as_str()).unwrap_or("?");
                                    let msg = r.get("message").and_then(|v| v.as_str()).unwrap_or("");
                                    println!("[{}] {}", from, msg);
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Status { pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    match http.get(format!("{}/api/panes/{}/status", base, pane_id)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                print_object(&val);
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Spawn { agent, title, cwd, role, parent, can_spawn, task, args, workspace } => {
                    let mut body = serde_json::json!({
                        "agent": agent,
                    });
                    if let Some(t) = title { body["title"] = serde_json::json!(t); }
                    if let Some(c) = cwd { body["cwd"] = serde_json::json!(c); }
                    if let Some(r) = role { body["role"] = serde_json::json!(r); }
                    if let Some(p) = parent { body["parent_pane_id"] = serde_json::json!(p); }
                    if can_spawn { body["can_spawn"] = serde_json::json!(true); }
                    if let Some(t) = task { body["task"] = serde_json::json!(t); }
                    if let Some(a) = args { body["args"] = serde_json::json!(a); }
                    if let Some(w) = workspace { body["workspace_id"] = serde_json::json!(w); }
                    // Use socket API (not REST) — avoids tokio runtime contention
                    match client.call("agent.spawn", Some(body)) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                if let Some(pid) = val.get("pane_id").and_then(|v| v.as_str()) {
                                    println!("Spawned: {} (pane {})", val.get("title").and_then(|v| v.as_str()).unwrap_or("?"), pid);
                                } else if let Some(err) = val.get("error").or_else(|| val.get("message")) {
                                    eprintln!("Error: {}", err);
                                } else {
                                    print_object(&val);
                                }
                                if let Some(warning) = val.get("warning").and_then(|v| v.as_str()) {
                                    eprintln!("Warning: {}", warning);
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Compact { pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    let cmd = "/compact\n";
                    match http.post(format!("{}/api/panes/{}/send", base, pane_id))
                        .json(&serde_json::json!({"text": cmd}))
                        .send()
                    {
                        Ok(_) => { println!("Sent /compact to {}", pane_id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Restart { pane, force, grace_secs } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    let body = serde_json::json!({"force": force, "grace_secs": grace_secs});
                    match http.post(format!("{}/api/panes/{}/agent/restart", base, pane_id))
                        .json(&body)
                        .send()
                    {
                        Ok(resp) => {
                            let status = resp.status();
                            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                                let retry = resp.headers()
                                    .get("retry-after")
                                    .and_then(|v| v.to_str().ok())
                                    .unwrap_or("30");
                                eprintln!("Error: rate limited (retry after {}s)", retry);
                                std::process::exit(1);
                            }
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if !status.is_success() {
                                let msg = val.get("error").and_then(|v| v.as_str())
                                    .unwrap_or_else(|| val.as_str().unwrap_or("restart failed"));
                                eprintln!("Error: {}", msg);
                                std::process::exit(1);
                            }
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("Restarting pane {}...", pane_id);
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Model { name, pane } => {
                    let valid = ["sonnet", "haiku", "opus"];
                    if !valid.contains(&name.as_str()) {
                        eprintln!("Invalid model: {}. Use: sonnet, haiku, opus", name);
                        std::process::exit(1);
                    }
                    let pane_id = resolve_pane(&mut client, pane);
                    let cmd = format!("/model {}\n", name);
                    match http.post(format!("{}/api/panes/{}/send", base, pane_id))
                        .json(&serde_json::json!({"text": cmd}))
                        .send()
                    {
                        Ok(_) => { println!("Sent /model {} to {}", name, pane_id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
            }
        }

        Commands::Theme { action } => {
            // Theme commands use REST API (port 7685)
            let base = "http://127.0.0.1:7685";
            let http = reqwest::blocking::Client::new();
            match action {
                ThemeAction::List => {
                    match http.get(format!("{}/api/themes", base)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if let Some(themes) = val.get("themes").and_then(|v| v.as_array()) {
                                for t in themes {
                                    println!("{}", t.as_str().unwrap_or("?"));
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                ThemeAction::Apply { name } => {
                    match http.post(format!("{}/api/themes/apply", base))
                        .json(&serde_json::json!({ "name": name }))
                        .send()
                    {
                        Ok(_) => { println!("Theme applied: {}", name); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                ThemeAction::Save { id, file, apply } => {
                    let toml = match std::fs::read_to_string(&file) {
                        Ok(c) => c,
                        Err(e) => { eprintln!("Error reading {}: {}", file, e); std::process::exit(1); }
                    };
                    match http.post(format!("{}/api/themes", base))
                        .json(&serde_json::json!({ "id": id, "toml": toml }))
                        .send()
                    {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
                                println!("Theme saved: {}", path);
                            } else {
                                println!("Theme saved: {}", id);
                            }
                            if apply {
                                match http.post(format!("{}/api/themes/apply", base))
                                    .json(&serde_json::json!({ "name": id }))
                                    .send()
                                {
                                    Ok(_) => println!("Theme applied: {}", id),
                                    Err(e) => eprintln!("Theme saved but failed to apply: {}", e),
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                ThemeAction::Delete { id } => {
                    match http.delete(format!("{}/api/themes/{}", base, id)).send() {
                        Ok(_) => { println!("Theme deleted: {}", id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                ThemeAction::Dir => {
                    let dir = dirs::config_dir()
                        .map(|d| d.join("codeterm").join("themes"))
                        .unwrap_or_default();
                    println!("{}", dir.display());
                    return;
                }
            }
        }

        Commands::Cron { action } => {
            // Cron uses socket API
            match action {
                CronAction::List { workspace } => {
                    let mut params = serde_json::json!({});
                    if let Some(ws) = workspace { params["workspace_id"] = serde_json::json!(ws); }
                    match client.call("cron.list", Some(params)) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() { println!("No cron jobs."); }
                                for j in arr {
                                    let enabled = j.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                                    let oneshot = j.get("oneshot").and_then(|v| v.as_bool()).unwrap_or(false);
                                    let status = match (enabled, oneshot) {
                                        (_, true) => "oneshot",
                                        (true, false) => "enabled",
                                        (false, false) => "disabled",
                                    };
                                    println!("{} {} [{}] schedule={} agent={}",
                                        j.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                                        j.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                                        status,
                                        j.get("schedule").and_then(|v| v.as_str()).unwrap_or("?"),
                                        j.get("agent").and_then(|v| v.as_str()).unwrap_or("?"),
                                    );
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Create { name, schedule, task, workspace, agent, model, cwd, description, creator_pane, once } => {
                    let ws_id = workspace.unwrap_or_else(|| {
                        // Prefer CODETERM_WORKSPACE_ID (agent's own workspace), fall back to first
                        std::env::var("CODETERM_WORKSPACE_ID").ok()
                            .unwrap_or_else(|| {
                                client.call("workspace.list", None)
                                    .ok()
                                    .and_then(|v| v.as_array().and_then(|a| a.first().cloned()))
                                    .and_then(|w| w.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                                    .unwrap_or_default()
                            })
                    });
                    let mut params = serde_json::json!({
                        "name": name,
                        "schedule": schedule,
                        "task": task,
                        "workspace_id": ws_id,
                        "agent": agent,
                    });
                    if let Some(m) = model { params["model"] = serde_json::json!(m); }
                    if let Some(c) = cwd { params["cwd"] = serde_json::json!(c); }
                    if let Some(d) = description { params["description"] = serde_json::json!(d); }
                    if let Some(cp) = creator_pane { params["creator_pane_id"] = serde_json::json!(cp); }
                    if once { params["oneshot"] = serde_json::json!(true); }
                    match client.call("cron.create", Some(params)) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("Created: {} ({})",
                                    val.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                                    val.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                                );
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Edit { id, name, schedule, task, cwd, description, agent, model, jitter_max_ms } => {
                    let mut params = serde_json::json!({"id": id});
                    if let Some(n) = name { params["name"] = serde_json::json!(n); }
                    if let Some(s) = schedule { params["schedule"] = serde_json::json!(s); }
                    if let Some(t) = task { params["task"] = serde_json::json!(t); }
                    if let Some(c) = cwd { params["cwd"] = serde_json::json!(c); }
                    if let Some(d) = description { params["description"] = serde_json::json!(d); }
                    if let Some(a) = agent { params["agent"] = serde_json::json!(a); }
                    if let Some(m) = model { params["model"] = serde_json::json!(m); }
                    if let Some(j) = jitter_max_ms { params["jitter_max_ms"] = serde_json::json!(j); }
                    match client.call("cron.update", Some(params)) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("Updated: {}", val.get("name").and_then(|v| v.as_str()).unwrap_or("?"));
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Delete { id } => {
                    match client.call("cron.delete", Some(serde_json::json!({"id": id}))) {
                        Ok(_) => { println!("Deleted: {}", id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Enable { id } => {
                    match client.call("cron.enable", Some(serde_json::json!({"id": id}))) {
                        Ok(_) => { println!("Enabled: {}", id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Disable { id } => {
                    match client.call("cron.disable", Some(serde_json::json!({"id": id}))) {
                        Ok(_) => { println!("Disabled: {}", id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Trigger { id } => {
                    match client.call("cron.trigger", Some(serde_json::json!({"id": id}))) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("Triggered: pane={}",
                                    val.get("pane_id").and_then(|v| v.as_str()).unwrap_or("?"));
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Cancel { id } => {
                    match client.call("cron.cancel", Some(serde_json::json!({"id": id}))) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                let n = val.get("cancelled").and_then(|v| v.as_u64()).unwrap_or(0);
                                if n == 0 {
                                    println!("No running executions for {}.", id);
                                } else {
                                    println!("Cancelled {} running execution(s) of {}.", n, id);
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::History { id } => {
                    match client.call("cron.history", Some(serde_json::json!({"id": id}))) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() { println!("No runs yet."); }
                                for r in arr {
                                    println!("{} {} started={} pane={}",
                                        r.get("run_id").and_then(|v| v.as_str()).unwrap_or("?"),
                                        r.get("status").and_then(|v| v.as_str()).unwrap_or("?"),
                                        r.get("started_at").and_then(|v| v.as_u64()).unwrap_or(0),
                                        r.get("pane_id").and_then(|v| v.as_str()).unwrap_or("-"),
                                    );
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Complete { run_id, summary } => {
                    let mut params = serde_json::json!({"run_id": run_id});
                    if let Some(s) = summary { params["summary"] = serde_json::json!(s); }
                    match client.call("cron.complete", Some(params)) {
                        Ok(_) => { println!("Completed: {}", run_id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
            }
        }

        Commands::Update { action, check, apply, channel, manifest_url } => {
            let (base_url, token) = read_api_config();
            let http = build_http(&token);

            let resolved = if let Some(ch) = channel {
                Some(UpdateAction::Channel {
                    channel: if ch.is_empty() { None } else { Some(ch) },
                })
            } else if apply {
                Some(UpdateAction::Install { auto: true, manifest_url: manifest_url.clone() })
            } else if check {
                Some(UpdateAction::Check { force: false })
            } else {
                action
            };

            match resolved.unwrap_or(UpdateAction::Check { force: false }) {
                UpdateAction::Check { force } => {
                    let url = if force {
                        format!("{}/api/update/check?force=true", base_url)
                    } else {
                        format!("{}/api/update/check", base_url)
                    };
                    match http.get(&url).send() {
                        Ok(resp) if resp.status().is_success() => {
                            let body: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                            } else {
                                let current = body["current_version"].as_str().unwrap_or("?");
                                let latest = body["latest_version"].as_str().unwrap_or("?");
                                let method = body["install_method"].as_str().unwrap_or("unknown");
                                let available = body["update_available"].as_bool().unwrap_or(false);

                                println!("Current: v{}  Latest: v{}  Install: {}", current, latest, method);
                                if available {
                                    println!("Update available!");
                                    if let Some(cmd) = body["update_command"].as_str() {
                                        println!("Run: {}", cmd);
                                    } else if let Some(url) = body["download_url"].as_str() {
                                        println!("Download: {}", url);
                                    }
                                } else {
                                    println!("Up to date.");
                                }
                            }
                        }
                        Ok(resp) => {
                            eprintln!("Error: {}", resp.status());
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    return;
                }
                UpdateAction::Install { auto, manifest_url } => {
                    let mut params: Vec<(&str, String)> = Vec::new();
                    if auto {
                        params.push(("method", "auto".into()));
                    }
                    if let Some(m) = manifest_url {
                        params.push(("manifest_url", m));
                    }
                    let qs = if params.is_empty() {
                        String::new()
                    } else {
                        let pairs: Vec<String> = params
                            .iter()
                            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
                            .collect();
                        format!("?{}", pairs.join("&"))
                    };
                    let url = format!("{}/api/update/install{}", base_url, qs);
                    match http.post(&url).send() {
                        Ok(resp) if resp.status().is_success() => {
                            let body: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                            } else {
                                let status = body["status"].as_str().unwrap_or("unknown");
                                match status {
                                    "up_to_date" => println!("Already up to date."),
                                    "command" => {
                                        if let Some(cmd) = body["command"].as_str() {
                                            println!("{}", cmd);
                                        }
                                    }
                                    "browser_opened" => {
                                        println!("Opened download page in browser.");
                                    }
                                    "updating" => {
                                        if let Some(msg) = body["message"].as_str() {
                                            println!("{}", msg);
                                        }
                                    }
                                    _ => println!("{}", body),
                                }
                            }
                        }
                        Ok(resp) => {
                            eprintln!("Error: {}", resp.status());
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    return;
                }
                UpdateAction::Channel { channel } => {
                    let url = format!("{}/api/update/channel", base_url);
                    let result = match channel {
                        None => http.get(&url).send(),
                        Some(name) => http
                            .post(&url)
                            .json(&serde_json::json!({ "channel": name }))
                            .send(),
                    };
                    match result {
                        Ok(resp) if resp.status().is_success() => {
                            let body: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                            } else if let Some(c) = body["channel"].as_str() {
                                println!("Channel: {}", c);
                            } else {
                                println!("{}", body);
                            }
                        }
                        Ok(resp) => {
                            eprintln!("Error: {}", resp.status());
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    return;
                }
            }
        }

        // Config is handled before socket connect (uses HTTP API)
        Commands::Config { .. } => unreachable!(),


        Commands::Mesh { action } => {
            let (base, token) = read_api_config();
            let http = build_http(&token);
            match action {
                MeshAction::Status => {
                    match http.get(format!("{}/api/mesh/status", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                let name = val.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                let id = val.get("instance_id").and_then(|v| v.as_str()).unwrap_or("?");
                                let paired = val.get("paired_count").and_then(|v| v.as_u64()).unwrap_or(0);
                                let discovered = val.get("discovered_count").and_then(|v| v.as_u64()).unwrap_or(0);
                                println!("{} ({})", name, &id[..id.len().min(8)]);
                                println!("Paired: {}, Discovered: {}", paired, discovered);
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Peers => {
                    match http.get(format!("{}/api/mesh/peers", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                if let Some(paired) = val.get("paired").and_then(|v| v.as_array()) {
                                    if !paired.is_empty() { println!("Paired:"); }
                                    for p in paired {
                                        let name = p.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                        let online = p.get("online").and_then(|v| v.as_bool()).unwrap_or(false);
                                        let ip = p.get("addresses").and_then(|v| v.as_array())
                                            .and_then(|a| a.first())
                                            .and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("?");
                                        let dot = if online { "●" } else { "○" };
                                        println!("  {} {} ({}) {}", dot, name, ip,
                                            if p.get("is_primary").and_then(|v| v.as_bool()).unwrap_or(false) { "★" } else { "" });
                                    }
                                }
                                if let Some(discovered) = val.get("discovered").and_then(|v| v.as_array()) {
                                    if !discovered.is_empty() { println!("Discovered:"); }
                                    for d in discovered {
                                        let name = d.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                        let method = d.get("discovery_method").and_then(|v| v.as_str()).unwrap_or("?");
                                        let ip = d.get("addresses").and_then(|v| v.as_array())
                                            .and_then(|a| a.first())
                                            .and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("?");
                                        let dev = d.get("dev").and_then(|v| v.as_bool()).unwrap_or(false);
                                        println!("  ◇ {} ({}) [{}]{}", name, ip, method, if dev { " [dev]" } else { "" });
                                    }
                                }
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Exec { target, command, pane } => {
                    let (action, target, command, pane) = ("exec", target, command, pane);
                    mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, action, &command, &pane, cli.json);
                    return;
                }
                MeshAction::Send { target, text, pane } => {
                    mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, "send", &text, &pane, cli.json);
                    return;
                }
                MeshAction::Output { target, pane } => {
                    mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, "output", "", &pane, cli.json);
                    return;
                }
                MeshAction::Panes { target } => {
                    mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, "panes", "", "", cli.json);
                    return;
                }
                MeshAction::Connect { instance_id } => {
                    let body = serde_json::json!({"instance_id": instance_id});
                    match http.post(format!("{}/api/mesh/connect", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .json(&body).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if val.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                                println!("Pairing request sent to {}", instance_id);
                                if let Some(rid) = val.get("request_id").and_then(|v| v.as_str()) {
                                    println!("Request ID: {}", rid);
                                }
                            } else {
                                let err = val.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                                eprintln!("Error: {}", err);
                                std::process::exit(1);
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Approve { request_id } => {
                    let body = serde_json::json!({"request_id": request_id});
                    match http.post(format!("{}/api/mesh/approve", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .json(&body).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if val.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                                println!("Pairing request {} approved", request_id);
                            } else {
                                let err = val.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                                eprintln!("Error: {}", err);
                                std::process::exit(1);
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Reject { request_id } => {
                    let body = serde_json::json!({"request_id": request_id});
                    match http.post(format!("{}/api/mesh/reject", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .json(&body).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if val.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                                println!("Pairing request {} rejected and blocked", request_id);
                            } else {
                                let err = val.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                                eprintln!("Error: {}", err);
                                std::process::exit(1);
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Disconnect { instance_id } => {
                    let body = serde_json::json!({"instance_id": instance_id});
                    match http.post(format!("{}/api/mesh/disconnect", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .json(&body).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if val.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                                println!("Disconnected from {}", instance_id);
                            } else {
                                let err = val.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                                eprintln!("Error: {}", err);
                                std::process::exit(1);
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Discover => {
                    match http.post(format!("{}/api/mesh/discover", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() {
                                    println!("No discovered peers");
                                } else {
                                    for p in arr {
                                        let name = p.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                        let id = p.get("instance_id").and_then(|v| v.as_str()).unwrap_or("?");
                                        let method = p.get("discovery_method").and_then(|v| v.as_str()).unwrap_or("?");
                                        println!("  {} ({}) [{}]", name, id, method);
                                    }
                                }
                            } else {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Broadcast => {
                    match http.post(format!("{}/api/mesh/broadcast/start", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(dur) = val.get("duration_secs").and_then(|v| v.as_u64()) {
                                println!("Broadcasting for {}s", dur);
                            } else {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::StopBroadcast => {
                    match http.post(format!("{}/api/mesh/broadcast/stop", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("Broadcast stopped");
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Refresh => {
                    match http.post(format!("{}/api/mesh/refresh", base)).send() {
                        Ok(resp) if resp.status().is_success() => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                let ts = val.get("tailscale_ip").and_then(|v| v.as_str()).unwrap_or("(none)");
                                let lan = val.get("local_ip").and_then(|v| v.as_str()).unwrap_or("(none)");
                                println!("Local addresses re-detected:");
                                println!("  tailscale: {}", ts);
                                println!("  lan:       {}", lan);
                                if let Some(rep) = val.get("announce") {
                                    let sent = rep.get("sent").and_then(|v| v.as_u64()).unwrap_or(0);
                                    let ok = rep.get("ok").and_then(|v| v.as_u64()).unwrap_or(0);
                                    println!("Announced to {}/{} paired peers", ok, sent);
                                    if let Some(stale) = rep.get("stale_peers").and_then(|v| v.as_array()) {
                                        if !stale.is_empty() {
                                            println!("Stale peers (need manual update — too old to recognize announce):");
                                            for s in stale { println!("  - {}", s.as_str().unwrap_or("?")); }
                                        }
                                    }
                                    if let Some(errs) = rep.get("errors").and_then(|v| v.as_array()) {
                                        if !errs.is_empty() {
                                            println!("Errors:");
                                            for e in errs { println!("  - {}", e.as_str().unwrap_or("?")); }
                                        }
                                    }
                                }
                            }
                        }
                        Ok(resp) => {
                            let status = resp.status();
                            let body = resp.text().unwrap_or_default();
                            eprintln!("Error ({}): {}", status, body);
                            std::process::exit(1);
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Announce { to } => {
                    let url = match to {
                        Some(t) => format!("{}/api/mesh/announce/all?to={}", base, urlencoding::encode(&t)),
                        None => format!("{}/api/mesh/announce/all", base),
                    };
                    match http.post(&url).send() {
                        Ok(resp) if resp.status().is_success() => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                let rep = val.get("report").cloned().unwrap_or(serde_json::Value::Null);
                                let sent = rep.get("sent").and_then(|v| v.as_u64()).unwrap_or(0);
                                let ok = rep.get("ok").and_then(|v| v.as_u64()).unwrap_or(0);
                                println!("Announced to {}/{} paired peers", ok, sent);
                                if let Some(stale) = rep.get("stale_peers").and_then(|v| v.as_array()) {
                                    if !stale.is_empty() {
                                        println!("Stale peers (need manual update):");
                                        for s in stale { println!("  - {}", s.as_str().unwrap_or("?")); }
                                    }
                                }
                                if let Some(errs) = rep.get("errors").and_then(|v| v.as_array()) {
                                    if !errs.is_empty() {
                                        println!("Errors:");
                                        for e in errs { println!("  - {}", e.as_str().unwrap_or("?")); }
                                    }
                                }
                            }
                        }
                        Ok(resp) => {
                            let status = resp.status();
                            let body = resp.text().unwrap_or_default();
                            eprintln!("Error ({}): {}", status, body);
                            std::process::exit(1);
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Peer { action } => match action {
                    MeshPeerAction::SetAddr { peer, addr } => {
                        let (ip, port) = mesh_cli::parse_ip_port(&addr);
                        if ip.is_empty() {
                            eprintln!("Error: invalid address '{}'", addr);
                            std::process::exit(1);
                        }
                        let mut body = serde_json::json!({"ip": ip});
                        if let Some(p) = port { body["port"] = serde_json::json!(p); }
                        let url = format!("{}/api/mesh/peers/{}/addr", base, urlencoding::encode(&peer));
                        match http.post(&url).json(&body).send() {
                            Ok(resp) if resp.status().is_success() => {
                                let val: serde_json::Value = resp.json().unwrap_or_default();
                                if cli.json {
                                    println!("{}", serde_json::to_string_pretty(&val).unwrap());
                                } else {
                                    let prev = val.get("previous").and_then(|v| v.as_array())
                                        .map(|a| a.iter().filter_map(|x| x.get("ip").and_then(|v| v.as_str())).collect::<Vec<_>>().join(", "))
                                        .unwrap_or_default();
                                    println!("Set {} → {}{}",
                                        peer,
                                        ip,
                                        port.map(|p| format!(":{}", p)).unwrap_or_default()
                                    );
                                    if !prev.is_empty() {
                                        println!("  was: {}", prev);
                                    }
                                }
                            }
                            Ok(resp) => {
                                let status = resp.status();
                                let body = resp.text().unwrap_or_default();
                                eprintln!("Error ({}): {}", status, body);
                                std::process::exit(1);
                            }
                            Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                        }
                        return;
                    }
                },
            }
        }
        Commands::Mem { action } => {
            if let MemAction::RunWorker { kind, db_path } = action {
                run_mem_worker(&kind, db_path.as_deref());
                return;
            }
            let json_mode = cli.json;
            handle_mem(json_mode, action);
            return;
        }
        Commands::Sessions { action } => {
            let (base_url, token) = read_api_config();
            let http = build_http(&token);
            handle_sessions(cli.json, action, &http, &base_url, token.as_deref());
            return;
        }
    };

    handle_result(cli.json, result);
}

// ── Mem CLI handler ─────────────────────────────────────────────────

/// Lightweight frontmatter parser for SKILL.md.
/// Recognises `title:`, `area:`, `tags:` (comma-separated) between leading `---` lines.
fn strip_skill_frontmatter(body: &str) -> &str {
    let mut lines = body.lines();
    if lines.next().map(|l| l.trim()) != Some("---") { return body; }
    let mut consumed = body.lines().next().unwrap_or("").len() + 1;
    for line in lines {
        consumed += line.len() + 1;
        if line.trim() == "---" {
            return body.get(consumed..).unwrap_or("");
        }
    }
    body
}

fn parse_skill_frontmatter(body: &str) -> (Option<String>, Option<String>, Vec<String>) {
    let mut lines = body.lines();
    if lines.next().map(|l| l.trim()) != Some("---") {
        return (None, None, Vec::new());
    }
    let mut title = None;
    let mut area = None;
    let mut tags = Vec::new();
    for line in lines {
        let t = line.trim();
        if t == "---" { break; }
        if let Some(v) = t.strip_prefix("title:") {
            title = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = t.strip_prefix("area:") {
            area = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = t.strip_prefix("tags:") {
            tags = v.trim().trim_matches('[').trim_matches(']')
                .split(',').map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty()).collect();
        }
    }
    (title, area, tags)
}

fn mem_get_request(http: &reqwest::blocking::Client, url: &str) -> Result<serde_json::Value, String> {
    let resp = http.get(url).send().map_err(|e| e.to_string())?;
    let status = resp.status();
    let val: serde_json::Value = resp.json().unwrap_or_default();
    if !status.is_success() {
        return Err(val.get("message").and_then(|v| v.as_str()).map(String::from)
            .unwrap_or_else(|| format!("HTTP {}", status)));
    }
    Ok(val)
}

fn mem_post_request(
    http: &reqwest::blocking::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let resp = http.post(url).json(body).send().map_err(|e| e.to_string())?;
    let status = resp.status();
    let val: serde_json::Value = resp.json().unwrap_or_default();
    if !status.is_success() {
        return Err(val.to_string());
    }
    Ok(val)
}

fn handle_mem(json_mode: bool, action: MemAction) {
    use std::io::Read;
    let (base, token) = read_api_config();
    let http = build_http(&token);

    let result: Result<serde_json::Value, String> = match action {
        MemAction::Search { query, kind, area, scope, limit, rerank } => {
            let mut url = format!("{}/api/mem/search?q={}&limit={}",
                base, urlencoding::encode(&query), limit);
            if let Some(k) = &kind { url.push_str(&format!("&kind={}", k)); }
            if let Some(a) = &area { url.push_str(&format!("&area={}", a)); }
            if let Some(s) = &scope { url.push_str(&format!("&scope={}", s)); }
            if rerank { url.push_str("&rerank=true"); }
            mem_get_request(&http, &url)
        }
        MemAction::Get { id } => {
            mem_get_request(&http, &format!("{}/api/mem/items/{}", base, id))
        }
        MemAction::Delete { id } => {
            let resp = http.delete(format!("{}/api/mem/items/{}", base, id))
                .send().map_err(|e| e.to_string());
            resp.map(|r| serde_json::json!({"deleted": r.status().is_success()}))
        }
        MemAction::History { id } => {
            let val = mem_get_request(&http, &format!("{}/api/mem/items/{}/history", base, id));
            // Pretty-print timeline in human mode; keep raw JSON for --json.
            if !json_mode {
                match &val {
                    Ok(v) => {
                        if let Some(rows) = v.get("history").and_then(|h| h.as_array()) {
                            print_history_timeline(rows);
                            return;
                        }
                    }
                    Err(_) => {}
                }
            }
            val
        }
        MemAction::Taxonomy { workspace } => {
            let mut url = format!("{}/api/mem/taxonomy", base);
            if let Some(ws) = &workspace {
                url.push_str(&format!("?workspace={}", urlencoding::encode(ws)));
            }
            let val = mem_get_request(&http, &url);
            if !json_mode {
                if let Ok(ref v) = val {
                    print_taxonomy(v);
                    return;
                }
            }
            val
        }
        MemAction::Tail { follow, limit } => {
            return run_mem_tail(&http, &base, json_mode, follow, limit);
        }
        MemAction::Update { id, title, summary, area, tags, body, importance } => {
            let tag_list: Option<Vec<String>> = tags.map(|t|
                t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
            );
            let payload = serde_json::json!({
                "title": title, "summary": summary, "area": area,
                "tags": tag_list, "body": body, "importance": importance,
            });
            let resp = http.patch(format!("{}/api/mem/items/{}", base, id))
                .json(&payload)
                .send()
                .map_err(|e| e.to_string());
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let val: serde_json::Value = r.json().unwrap_or_default();
                    if status.is_success() { Ok(val) } else { Err(val.to_string()) }
                }
                Err(e) => Err(e),
            }
        }
        MemAction::Save { kind, title, body, summary, tags, area, importance, scope, source_path, replace, skip_extract } => {
            // Body: positional arg, else read stdin if piped
            let resolved_body = match body {
                Some(b) => b,
                None => {
                    let mut s = String::new();
                    if std::io::stdin().read_to_string(&mut s).is_err() || s.trim().is_empty() {
                        eprintln!("Error: no body provided (pass as arg or pipe to stdin)");
                        std::process::exit(1);
                    }
                    s
                }
            };
            let resolved_title = title.unwrap_or_else(|| {
                resolved_body.lines().next().unwrap_or("untitled").chars().take(80).collect()
            });
            let split_csv = |raw: Option<String>| -> Vec<String> {
                raw.map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                    .unwrap_or_default()
            };
            let tag_list = split_csv(tags);
            let payload = serde_json::json!({
                "kind": kind,
                "title": resolved_title,
                "summary": summary,
                "area": area,
                "tags": tag_list,
                "body": resolved_body,
                "importance": importance,
                "scope": scope,
                "source_path": source_path,
                "replace": replace,
                "skip_extract": skip_extract,
            });
            mem_post_request(&http, &format!("{}/api/mem/items", base), &payload)
        }
        MemAction::ExtractPending { limit } => {
            mem_post_request(
                &http,
                &format!("{}/api/mem/extract-pending?limit={}", base, limit),
                &serde_json::json!({}),
            )
        }
        MemAction::Skill { action } => match action {
            MemSkillAction::New { name, area } => {
                let skills_root = dirs::home_dir().unwrap_or_default().join(data_dir_name()).join("skills");
                let folder_path = skills_root.join(&name);
                if folder_path.exists() {
                    eprintln!("Error: skill folder already exists: {}", folder_path.display());
                    std::process::exit(1);
                }
                if let Err(e) = std::fs::create_dir_all(folder_path.join("scripts")) {
                    eprintln!("Error creating folder: {}", e);
                    std::process::exit(1);
                }
                let area_line = area.as_deref().unwrap_or("general");
                let template = format!(
                    "---\ntitle: {name}\narea: {area_line}\ntags: []\n---\n\n# {name}\n\n_One-paragraph summary that becomes the search snippet._\n\n## When to use\n\n…\n\n## Steps\n\n1. …\n2. …\n\n## Gotchas\n\n…\n\n## Scripts\n\n- `./scripts/foo.sh` — describe what it does\n",
                    name = name, area_line = area_line,
                );
                let skill_md = folder_path.join("SKILL.md");
                if let Err(e) = std::fs::write(&skill_md, template) {
                    eprintln!("Error writing SKILL.md: {}", e);
                    std::process::exit(1);
                }
                eprintln!("Created skill folder: {}", folder_path.display());
                eprintln!("Edit: {}", skill_md.display());
                eprintln!("Add scripts to: {}", folder_path.join("scripts").display());
                eprintln!();
                // Register immediately so it appears in mem search right away.
                let body = std::fs::read_to_string(&skill_md).unwrap_or_default();
                let (fm_title, fm_area, _fm_tags) = parse_skill_frontmatter(&body);
                let title = fm_title.unwrap_or(name.clone());
                let area_resolved = area.clone().or(fm_area);
                let summary = format!("Skill scaffold — edit {} to fill in.", skill_md.display());
                let source = format!("file:{}", skill_md.display());
                let mut payload = serde_json::json!({
                    "kind": "skill",
                    "title": title,
                    "body": body,
                    "summary": summary,
                    "source": source,
                    "source_path": source,
                    "tags": ["folder-skill", "scaffold"],
                    "importance": 4.0,
                    "skip_extract": true,
                    "replace": true,
                });
                if let Some(a) = area_resolved {
                    payload["area"] = serde_json::json!(a);
                }
                mem_post_request(&http, &format!("{}/api/mem/items", base), &payload)
            }
            MemSkillAction::Add { folder, area, importance } => {
                let folder_expanded = if let Some(stripped) = folder.strip_prefix("~/") {
                    dirs::home_dir().map(|h| h.join(stripped)).unwrap_or_else(|| std::path::PathBuf::from(&folder))
                } else if folder == "~" {
                    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("~"))
                } else {
                    std::path::PathBuf::from(&folder)
                };
                let folder_path = folder_expanded;
                let skill_md = folder_path.join("SKILL.md");
                if !skill_md.exists() {
                    eprintln!("Error: SKILL.md not found in {}", folder_path.display());
                    std::process::exit(1);
                }
                let body = match std::fs::read_to_string(&skill_md) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error reading SKILL.md: {}", e);
                        std::process::exit(1);
                    }
                };
                // Optional YAML-ish frontmatter parse: between two --- lines at top.
                // Picks `title:`, `area:`, `tags:` (comma list) if present.
                let (fm_title, fm_area, fm_tags) = parse_skill_frontmatter(&body);
                // Title priority: frontmatter → first H1 → folder name.
                let title = fm_title.or_else(|| body.lines()
                    .find(|l| l.trim_start().starts_with("# "))
                    .map(|l| l.trim_start().trim_start_matches("# ").trim().to_string()))
                    .unwrap_or_else(|| folder_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "skill".into()));
                // Area priority: --area flag → frontmatter → parent folder name (if grandparent is skills/).
                let area_resolved = area.clone().or(fm_area).or_else(|| {
                    let parent = folder_path.parent()?.file_name()?.to_string_lossy().to_string();
                    if parent == "skills" { None } else { Some(parent) }
                });
                // Skip the YAML frontmatter block before picking summary.
                let body_after_fm = strip_skill_frontmatter(&body);
                let summary: String = body_after_fm.lines()
                    .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                    .next().unwrap_or("").to_string();
                let source = format!("file:{}", skill_md.display());
                let mut tags: Vec<String> = vec!["folder-skill".into()];
                tags.extend(fm_tags);
                let mut payload = serde_json::json!({
                    "kind": "skill",
                    "title": title,
                    "body": body,
                    "summary": summary,
                    "source": source,
                    "source_path": source,
                    "tags": tags,
                    "importance": importance,
                    "skip_extract": true,
                    "replace": true,
                });
                if let Some(a) = area_resolved {
                    payload["area"] = serde_json::json!(a);
                }
                mem_post_request(&http, &format!("{}/api/mem/items", base), &payload)
            }
            MemSkillAction::List => {
                match mem_get_request(&http, &format!("{}/api/mem/tail?limit=500", base)) {
                    Ok(resp) => {
                        if let Some(rows) = resp.get("rows").and_then(|v| v.as_array()) {
                            let skills: Vec<&serde_json::Value> = rows.iter()
                                .filter(|r| r.get("kind").and_then(|k| k.as_str()) == Some("skill"))
                                .collect();
                            Ok(serde_json::json!({"skills": skills}))
                        } else {
                            Ok(resp)
                        }
                    }
                    Err(e) => Err(e),
                }
            }
        },
        MemAction::WakeUp { budget, l0_only, no_l0 } => {
            let mut url = format!("{}/api/mem/wake-up", base);
            let mut sep = '?';
            if let Some(b) = budget {
                url.push_str(&format!("{}budget={}", sep, b));
                sep = '&';
            }
            if l0_only { url.push_str(&format!("{}l0_only=true", sep)); sep = '&'; }
            if no_l0 { url.push_str(&format!("{}no_l0=true", sep)); let _ = sep; }
            mem_get_request(&http, &url)
        }
        MemAction::Stats => {
            mem_get_request(&http, &format!("{}/api/mem/stats", base))
        }
        MemAction::Learn { text, kind, tags, error, feature } => {
            let (resolved_text, resolved_kind) = if let Some(t) = error {
                (t, "error".to_string())
            } else if let Some(t) = feature {
                (t, "feature".to_string())
            } else if let Some(t) = text {
                (t, kind.unwrap_or_else(|| "insight".into()))
            } else {
                let mut s = String::new();
                if std::io::stdin().read_to_string(&mut s).is_err() || s.trim().is_empty() {
                    eprintln!("Error: provide text as arg, --error/--feature, or pipe to stdin");
                    std::process::exit(1);
                }
                (s, kind.unwrap_or_else(|| "insight".into()))
            };
            let tag_list: Vec<String> = tags
                .map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            mem_post_request(&http, &format!("{}/api/mem/learn", base), &serde_json::json!({
                "text": resolved_text, "kind": resolved_kind, "tags": tag_list,
            }))
        }
        MemAction::LearnList => {
            mem_get_request(&http, &format!("{}/api/mem/search?q=learning&kind=learning&limit=50", base))
        }
        MemAction::LearnPromote { id } => {
            mem_post_request(&http, &format!("{}/api/mem/promote/{}", base, id), &serde_json::json!({}))
        }
        MemAction::Flush { pane, title } => {
            let pane_id = pane
                .or_else(|| std::env::var("CODETERM_PANE_ID").ok())
                .unwrap_or_else(|| "current".to_string());
            mem_post_request(&http, &format!("{}/api/mem/flush", base), &serde_json::json!({
                "pane": pane_id, "title": title,
            }))
        }
        MemAction::Dream => {
            mem_post_request(&http, &format!("{}/api/mem/dream", base), &serde_json::json!({}))
        }
        MemAction::Sync { path } => {
            let mut url = format!("{}/api/mem/sync", base);
            if let Some(p) = path {
                url.push_str(&format!("?path={}", urlencoding::encode(&p)));
            }
            mem_post_request(&http, &url, &serde_json::json!({}))
        }
        MemAction::Graph { action } => match action {
            MemGraphAction::Create { id, entity_type, props, scope } => {
                let props_val: serde_json::Value =
                    serde_json::from_str(&props).unwrap_or(serde_json::json!({}));
                let payload = serde_json::json!({
                    "id": id, "type": entity_type, "scope": scope, "properties": props_val,
                });
                mem_post_request(&http, &format!("{}/api/mem/graph/entities", base), &payload)
            }
            MemGraphAction::Relate { from, rel, to, valid_from, valid_to, confidence, scope } => {
                let payload = serde_json::json!({
                    "from": from, "rel": rel, "to": to,
                    "valid_from": valid_from, "valid_to": valid_to,
                    "confidence": confidence, "scope": scope,
                });
                mem_post_request(&http, &format!("{}/api/mem/graph/relate", base), &payload)
            }
            MemGraphAction::Related { id, rel, as_of } => {
                let mut url = format!("{}/api/mem/graph/related?id={}", base, urlencoding::encode(&id));
                if let Some(r) = &rel { url.push_str(&format!("&rel={}", r)); }
                if let Some(d) = &as_of { url.push_str(&format!("&as_of={}", urlencoding::encode(d))); }
                mem_get_request(&http, &url)
            }
            MemGraphAction::Invalidate { from, rel, to, ended } => {
                let payload = serde_json::json!({
                    "from": from, "rel": rel, "to": to, "ended": ended,
                });
                mem_post_request(&http, &format!("{}/api/mem/graph/invalidate", base), &payload)
            }
            MemGraphAction::Timeline { entity } => {
                let url = format!("{}/api/mem/graph/timeline?entity={}", base, urlencoding::encode(&entity));
                mem_get_request(&http, &url)
            }
            MemGraphAction::Traverse { from, hops, max_nodes, as_of } => {
                let mut url = format!("{}/api/mem/graph/traverse?from={}&hops={}&max_nodes={}",
                    base, urlencoding::encode(&from), hops, max_nodes);
                if let Some(d) = &as_of { url.push_str(&format!("&as_of={}", urlencoding::encode(d))); }
                mem_get_request(&http, &url)
            }
            MemGraphAction::Path { from, to, max_hops } => {
                let url = format!("{}/api/mem/graph/path?from={}&to={}&max_hops={}",
                    base, urlencoding::encode(&from), urlencoding::encode(&to), max_hops);
                mem_get_request(&http, &url)
            }
            MemGraphAction::Tunnels => {
                mem_get_request(&http, &format!("{}/api/mem/graph/tunnels", base))
            }
            MemGraphAction::Alias { alias, canonical } => {
                let payload = serde_json::json!({ "alias": alias, "canonical": canonical });
                mem_post_request(&http, &format!("{}/api/mem/graph/alias", base), &payload)
            }
        }
        MemAction::Worker { action } => match action {
            MemWorkerAction::Spawn { intent, task, agent, parent } => {
                let payload = serde_json::json!({
                    "intent": intent,
                    "task": task,
                    "agent": agent,
                    "parent": parent,
                });
                mem_post_request(&http, &format!("{}/api/mem/workers", base), &payload)
            }
            MemWorkerAction::List => {
                mem_get_request(&http, &format!("{}/api/mem/workers", base))
            }
            MemWorkerAction::Get { id } => {
                mem_get_request(&http, &format!("{}/api/mem/workers/{}", base, id))
            }
            MemWorkerAction::Progress { text, status } => {
                match std::env::var("CODETERM_WORKER_ID") {
                    Ok(id) => {
                        let payload = serde_json::json!({ "text": text, "status": status });
                        mem_post_request(&http, &format!("{}/api/mem/workers/{}/progress", base, id), &payload)
                    }
                    Err(_) => Err("not in a worker pane: $CODETERM_WORKER_ID is not set".into()),
                }
            }
            MemWorkerAction::Report { summary } => {
                match std::env::var("CODETERM_WORKER_ID") {
                    Ok(id) => {
                        let payload = serde_json::json!({ "summary": summary });
                        mem_post_request(&http, &format!("{}/api/mem/workers/{}/report", base, id), &payload)
                    }
                    Err(_) => Err("not in a worker pane: $CODETERM_WORKER_ID is not set".into()),
                }
            }
            MemWorkerAction::Kill { id } => {
                let resp = http.delete(format!("{}/api/mem/workers/{}", base, id))
                    .send().map_err(|e| e.to_string());
                resp.map(|r| serde_json::json!({"ok": r.status().is_success()}))
            }
        }
        MemAction::RunWorker { .. } => unreachable!("RunWorker is short-circuited in main()"),
    };

    // Wake-up returns plain text in `text` field — print it directly.
    if let Ok(ref val) = result {
        if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
            println!("{}", text);
            return;
        }
    }
    handle_result(json_mode, result);
}

fn handle_result(json: bool, result: Result<serde_json::Value, String>) {
    match result {
        Ok(value) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            } else {
                print_human(&value);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Print a JSON value in a human-readable format.
fn print_human(value: &serde_json::Value) {
    match value {
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                println!("(empty)");
                return;
            }
            for item in arr {
                print_object(item);
                println!();
            }
        }
        serde_json::Value::Object(_) => {
            print_object(value);
        }
        serde_json::Value::String(s) => println!("{}", s),
        serde_json::Value::Null => println!("OK"),
        other => println!("{}", other),
    }
}

fn print_object(value: &serde_json::Value) {
    if let Some(obj) = value.as_object() {
        let max_key_len = obj.keys().map(|k| k.len()).max().unwrap_or(0);
        for (key, val) in obj {
            let display_val = match val {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(a) => {
                    let items: Vec<String> = a
                        .iter()
                        .map(|v| match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect();
                    items.join(", ")
                }
                serde_json::Value::Null => "(none)".to_string(),
                other => other.to_string(),
            };
            println!("  {:width$}  {}", key, display_val, width = max_key_len);
        }
    }
}

// ── Phase C: history timeline rendering ─────────────────────────────
fn print_history_timeline(rows: &[serde_json::Value]) {
    if rows.is_empty() {
        println!("(no history)");
        return;
    }
    for row in rows {
        let changed_at = row.get("changed_at").and_then(|v| v.as_str()).unwrap_or("?");
        // Drop sub-second precision + tz for compact display.
        let ts = changed_at
            .split('.')
            .next()
            .unwrap_or(changed_at)
            .replace('T', " ");
        let changed_by = row.get("changed_by").and_then(|v| v.as_str()).unwrap_or("?");
        let change_type = row
            .get("change_type")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        println!("* {}  {:<15} {}", ts, changed_by, change_type);
        let diff_line = |label: &str, old: Option<&serde_json::Value>, new: Option<&serde_json::Value>| {
            let has_old = old.map(|v| !v.is_null()).unwrap_or(false);
            let has_new = new.map(|v| !v.is_null()).unwrap_or(false);
            if !has_old && !has_new {
                return None;
            }
            let fmt = |v: Option<&serde_json::Value>| -> String {
                match v {
                    None | Some(serde_json::Value::Null) => "-".into(),
                    Some(serde_json::Value::String(s)) => {
                        let truncated: String = s.chars().take(60).collect();
                        if s.chars().count() > 60 {
                            format!("{}…", truncated)
                        } else {
                            truncated
                        }
                    }
                    Some(other) => other.to_string(),
                }
            };
            if has_old && has_new {
                Some(format!("    {:<11} {} -> {}", format!("{}:", label), fmt(old), fmt(new)))
            } else if has_new {
                Some(format!("    {:<11} {}", format!("{}:", label), fmt(new)))
            } else {
                Some(format!("    {:<11} (deleted: {})", format!("{}:", label), fmt(old)))
            }
        };
        for (label, old_k, new_k) in [
            ("title", "old_title", "new_title"),
            ("kind", "old_kind", "new_kind"),
            ("importance", "old_importance", "new_importance"),
            ("body", "old_body", "new_body"),
        ] {
            if let Some(line) = diff_line(label, row.get(old_k), row.get(new_k)) {
                println!("{}", line);
            }
        }
        if let Some(reason) = row.get("reason").and_then(|v| v.as_str()) {
            println!("    reason:     {}", reason);
        }
    }
}

// ── mem tail ────────────────────────────────────────────────────────
fn run_mem_tail(
    http: &reqwest::blocking::Client,
    base: &str,
    json_mode: bool,
    follow: bool,
    limit: usize,
) {
    let fetch = |since: Option<&str>| -> Result<Vec<serde_json::Value>, String> {
        let mut url = format!("{}/api/mem/tail?limit={}", base, limit);
        if let Some(s) = since {
            url.push_str(&format!("&since={}", urlencoding::encode(s)));
        }
        let val = mem_get_request(http, &url)?;
        Ok(val
            .get("rows")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    };
    let print_row = |row: &serde_json::Value| {
        let ts = row
            .get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .split('.')
            .next()
            .unwrap_or("?")
            .replace('T', " ");
        let kind = row.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let who = row.get("changed_by").and_then(|v| v.as_str()).unwrap_or("?");
        let title = row.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        let area = row
            .get("area")
            .and_then(|v| v.as_str())
            .unwrap_or("(none)");
        println!(
            "{} | {:<12} | {:<15} | {:<60} | {}",
            ts,
            kind,
            who,
            title.chars().take(60).collect::<String>(),
            area
        );
    };

    match fetch(None) {
        Ok(rows) => {
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&rows).unwrap());
            } else if rows.is_empty() {
                println!("(no recent writes)");
            } else {
                // Oldest first so that follow-mode output streams naturally.
                for row in rows.iter().rev() {
                    print_row(row);
                }
            }
            if !follow {
                return;
            }
            // In follow mode we remember the newest updated_at we've printed
            // and poll with `since` every 500ms.
            let mut last_seen: Option<String> = rows
                .iter()
                .filter_map(|r| r.get("updated_at").and_then(|v| v.as_str()).map(String::from))
                .max();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let next = match fetch(last_seen.as_deref()) {
                    Ok(rows) => rows,
                    Err(e) => {
                        eprintln!("tail fetch: {}", e);
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        continue;
                    }
                };
                if !next.is_empty() {
                    for row in next.iter().rev() {
                        print_row(row);
                    }
                    if let Some(newest) = next
                        .iter()
                        .filter_map(|r| r.get("updated_at").and_then(|v| v.as_str()).map(String::from))
                        .max()
                    {
                        last_seen = Some(newest);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

// ── Phase D: taxonomy rendering ─────────────────────────────────────
fn print_taxonomy(value: &serde_json::Value) {
    let Some(obj) = value.as_object() else {
        print_human(value);
        return;
    };
    let mut areas: Vec<&String> = obj.keys().collect();
    // Alphabetical, with "(none)" pushed to the bottom.
    areas.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        ("(none)", "(none)") => std::cmp::Ordering::Equal,
        ("(none)", _) => std::cmp::Ordering::Greater,
        (_, "(none)") => std::cmp::Ordering::Less,
        (x, y) => x.cmp(y),
    });
    if areas.is_empty() {
        println!("(empty)");
        return;
    }
    for area in areas {
        println!("{}", area);
        if let Some(kinds) = obj.get(area).and_then(|v| v.as_object()) {
            let mut kind_keys: Vec<&String> = kinds.keys().collect();
            kind_keys.sort();
            for k in kind_keys {
                let n = kinds.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                println!("  {:<13} {}", k, n);
            }
        }
    }
}

// ── Sessions CLI handler ─────────────────────────────────────────────

/// Shell-quote a path for copy-pasteable `cd` hints.
fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "/_.-~".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Minimal unix-ts-to-"YYYY-MM-DD HH:MM" formatter (UTC). Avoids pulling
/// chrono into the CLI crate just for a date label.
fn format_unix_ts(ts: i64) -> String {
    if ts <= 0 {
        return "?".into();
    }
    let days = ts / 86_400;
    let sec_of_day = ts.rem_euclid(86_400);
    let hh = sec_of_day / 3600;
    let mm = (sec_of_day % 3600) / 60;
    // Civil-from-days algorithm (Howard Hinnant), days since 1970-01-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, hh, mm)
}

fn handle_sessions(
    json_mode: bool,
    action: SessionsAction,
    http: &reqwest::blocking::Client,
    base: &str,
    token: Option<&str>,
) {
    match action {
        SessionsAction::Search { query, provider, project, limit, max_snippets } => {
            let mut url = format!("{}/api/sessions/search?q={}", base, urlencoding::encode(&query));
            if let Some(p) = provider {
                url.push_str(&format!("&provider={}", urlencoding::encode(&p)));
            }
            if let Some(p) = project {
                url.push_str(&format!("&project={}", urlencoding::encode(&p)));
            }
            url.push_str(&format!("&limit={}", limit));
            url.push_str(&format!("&max_snippets={}", max_snippets));

            let mut req = http.get(&url);
            if let Some(t) = token {
                req = req.header("Authorization", format!("Bearer {}", t));
            }
            let resp = match req.send() {
                Ok(r) => r,
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            };
            let val: serde_json::Value = resp.json().unwrap_or_default();
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
                return;
            }
            let hits = val.get("hits").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            if hits.is_empty() {
                println!("No sessions matched: {}", query);
                return;
            }
            println!("Found {} session(s) for: {}\n", hits.len(), query);
            for (i, hit) in hits.iter().enumerate() {
                let meta = hit.get("meta").cloned().unwrap_or_default();
                let provider_id = meta.get("provider").and_then(|v| v.as_str()).unwrap_or("?");
                let sid = meta.get("session_id").and_then(|v| v.as_str()).unwrap_or("?");
                let project = meta.get("project_path").and_then(|v| v.as_str()).unwrap_or("");
                let title = meta.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let modified = meta.get("modified_ts").and_then(|v| v.as_i64()).unwrap_or(0);
                let modified_str = format_unix_ts(modified);
                println!("{:─<70}", "");
                let header = if title.is_empty() {
                    format!("{}. [{}] [{}] {}", i + 1, provider_id, modified_str, sid)
                } else {
                    format!("{}. [{}] [{}] {} — {}", i + 1, provider_id, modified_str, title, sid)
                };
                println!("  {}", header);
                if !project.is_empty() {
                    println!("  project: {}", project);
                }
                // Claude's `--resume` scans the CURRENT cwd's encoded project
                // dir, so you must `cd` to `project_path` first or the JSONL
                // won't be found even though we just located it. Include the
                // cd step in the hint when we know the path.
                let resume_cmd = match provider_id {
                    "claude" => format!("claude --resume {}", sid),
                    "codex" => format!("codex resume {}", sid),
                    other => format!("{} resume {}", other, sid),
                };
                let resume_hint = if !project.is_empty() {
                    format!("cd {} && {}", shell_quote(project), resume_cmd)
                } else {
                    resume_cmd
                };
                println!("  resume:  {}", resume_hint);
                println!();
                if let Some(snips) = hit.get("snippets").and_then(|v| v.as_array()) {
                    for s in snips {
                        let role = s.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                        let text = s.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        let tag = match role {
                            "user" => "You".to_string(),
                            "assistant" => match provider_id {
                                "claude" => "Claude".to_string(),
                                "codex" => "Codex".to_string(),
                                other => other.to_string(),
                            },
                            other => other.to_string(),
                        };
                        println!("  [{}] {}", tag, text);
                        println!();
                    }
                }
            }
        }
        SessionsAction::Resume { session_id, title, workspace_id, workspace_name } => {
            let mut body = serde_json::Map::new();
            if let Some(t) = title { body.insert("title".into(), serde_json::Value::String(t)); }
            if let Some(w) = workspace_id { body.insert("workspace_id".into(), serde_json::Value::String(w)); }
            if let Some(n) = workspace_name { body.insert("workspace_name".into(), serde_json::Value::String(n)); }

            let url = format!("{}/api/sessions/{}/resume", base, urlencoding::encode(&session_id));
            let mut req = http.post(&url).json(&serde_json::Value::Object(body));
            if let Some(t) = token {
                req = req.header("Authorization", format!("Bearer {}", t));
            }
            match req.send() {
                Ok(resp) => {
                    let status = resp.status();
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json_mode {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
                        return;
                    }
                    if !status.is_success() {
                        eprintln!("Error: {} — {}", status, val);
                        std::process::exit(1);
                    }
                    let pane = val.get("pane_id").and_then(|v| v.as_str()).unwrap_or("?");
                    let ws = val.get("workspace_id").and_then(|v| v.as_str()).unwrap_or("?");
                    let provider = val.get("provider").and_then(|v| v.as_str()).unwrap_or("?");
                    let cwd = val.get("cwd").and_then(|v| v.as_str()).unwrap_or("?");
                    println!("Resumed [{}] {}", provider, session_id);
                    println!("  workspace: {}", ws);
                    println!("  pane:      {}", pane);
                    println!("  cwd:       {}", cwd);
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        SessionsAction::Get { session_id } => {
            let url = format!("{}/api/sessions/{}", base, urlencoding::encode(&session_id));
            let mut req = http.get(&url);
            if let Some(t) = token {
                req = req.header("Authorization", format!("Bearer {}", t));
            }
            match req.send() {
                Ok(resp) => {
                    let status = resp.status();
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json_mode {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
                        return;
                    }
                    if !status.is_success() {
                        eprintln!("Error: {} — {}", status, val);
                        std::process::exit(1);
                    }
                    let provider = val.get("provider").and_then(|v| v.as_str()).unwrap_or("?");
                    let cwd = val.get("project_path").and_then(|v| v.as_str()).unwrap_or("-");
                    let file = val.get("file_path").and_then(|v| v.as_str()).unwrap_or("-");
                    let modified = val.get("modified_ts").and_then(|v| v.as_i64()).unwrap_or(0);
                    println!("session: {}", session_id);
                    println!("  provider: {}", provider);
                    println!("  cwd:      {}", cwd);
                    println!("  file:     {}", file);
                    println!("  modified: {}", format_unix_ts(modified));
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
    }
}

// ── `mem run-worker` subprocess ─────────────────────────────────────
//
// Opens knowledge.db directly (multi-process WAL) and runs one phase of
// dreaming or the librarian inline, then exits. The supervisor in the
// daemon spawns this so a panic / OOM / hang in the maintenance pass
// cannot take the API down.
//
// SQL is duplicated from src-tauri/src/mem/dreaming.rs / file_sync.rs on
// purpose — keeping the CLI free of the codeterm_lib dependency means it
// stays standalone and lightweight. The dreaming algorithm is stable;
// changes need to be applied here too.

fn run_mem_worker(kind: &str, db_path_arg: Option<&str>) {
    let db_path = match db_path_arg {
        Some(p) => std::path::PathBuf::from(p),
        None => match resolve_default_db_path() {
            Ok(p) => p,
            Err(e) => {
                worker_emit_error(&e);
                std::process::exit(1);
            }
        }
    };

    register_sqlite_vec_extension();

    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            worker_emit_error(&format!("open {}: {}", db_path.display(), e));
            std::process::exit(1);
        }
    };
    if let Err(e) = conn.busy_timeout(std::time::Duration::from_secs(5)) {
        worker_emit_error(&format!("busy_timeout: {}", e));
        std::process::exit(1);
    }
    if let Err(e) = conn.execute_batch("PRAGMA synchronous = NORMAL;") {
        worker_emit_error(&format!("synchronous pragma: {}", e));
        std::process::exit(1);
    }

    let result = match kind {
        "dreaming-light" => worker_dreaming_light(&conn).map(|r| serde_json::json!({
            "considered": r.considered, "promoted": r.promoted,
            "dropped": r.dropped, "phase": r.phase,
        })),
        "dreaming-deep" => worker_dreaming_deep(&conn).map(|r| serde_json::json!({
            "considered": r.considered, "promoted": r.promoted,
            "dropped": r.dropped, "phase": r.phase,
        })),
        "librarian-once" => {
            let skills_root = dirs::home_dir().unwrap_or_default()
                .join(data_dir_name()).join("skills");
            worker_librarian_once(&conn, &skills_root).map(|r| serde_json::json!(r))
        }
        other => Err(format!("unknown worker kind: {}", other)),
    };

    match result {
        Ok(report) => {
            println!("{}", serde_json::json!({"ok": true, "report": report}));
        }
        Err(e) => {
            worker_emit_error(&e);
            std::process::exit(1);
        }
    }
}

fn worker_emit_error(msg: &str) {
    println!("{}", serde_json::json!({"ok": false, "error": msg}));
}

fn resolve_default_db_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    Ok(home.join(data_dir_name()).join("mem").join("knowledge.db"))
}

fn register_sqlite_vec_extension() {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }
    });
}

#[derive(Debug, Clone)]
struct WorkerDreamReport {
    considered: u64,
    promoted: u64,
    dropped: u64,
    phase: String,
}

fn worker_dreaming_light(conn: &rusqlite::Connection) -> Result<WorkerDreamReport, String> {
    let now_ts = chrono::Utc::now();
    let one_day_ago = (now_ts - chrono::Duration::days(1)).to_rfc3339();

    let candidates: Vec<(String, i64, f64, String, Option<String>)> = {
        let mut stmt = conn.prepare(
            r#"SELECT id, access_count, importance, created_at, accessed_at
               FROM mem_items
               WHERE kind = 'learning'
                 AND access_count >= 3
                 AND created_at < ?1"#,
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![one_day_ago], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        }).map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let mut considered = 0u64;
    let mut promoted = 0u64;

    for (id, access_count, importance, created_at, accessed_at) in &candidates {
        considered += 1;

        let frequency = (*access_count as f64 / 10.0).min(1.0);
        let recency = accessed_at
            .as_ref()
            .and_then(|a| chrono::DateTime::parse_from_rfc3339(a).ok())
            .map(|a| {
                let days = (now_ts - a.with_timezone(&chrono::Utc))
                    .num_hours().max(0) as f64 / 24.0;
                (-days * 0.693 / 14.0).exp()
            })
            .unwrap_or(0.0);
        let imp_score = (*importance / 5.0).min(1.0);
        let age = chrono::DateTime::parse_from_rfc3339(created_at)
            .ok()
            .map(|c| {
                let days = (now_ts - c.with_timezone(&chrono::Utc))
                    .num_hours().max(0) as f64 / 24.0;
                (days / 7.0).min(1.0)
            })
            .unwrap_or(0.5);

        let score = frequency * 0.30 + recency * 0.25 + imp_score * 0.25 + age * 0.20;
        if score < 0.5 { continue; }

        let now = chrono::Utc::now().to_rfc3339();
        let n = conn.execute(
            "UPDATE mem_items SET kind = 'fact', importance = MAX(importance, 4.0), updated_at = ?1 WHERE id = ?2",
            rusqlite::params![&now, id],
        ).map_err(|e| e.to_string())?;
        if n > 0 {
            // history_log mirror — best-effort, don't fail the run on history errors.
            let new_imp: f64 = importance.max(4.0);
            let _ = conn.execute(
                r#"INSERT INTO mem_item_history
                   (item_id, changed_at, changed_by, change_type,
                    old_importance, new_importance, old_kind, new_kind)
                   VALUES (?1, ?2, 'dreaming:rem', 'promote', ?3, ?4, 'learning', 'fact')"#,
                rusqlite::params![id, &now, importance, new_imp],
            );
            promoted += 1;
        }
    }

    Ok(WorkerDreamReport {
        considered, promoted, dropped: 0,
        phase: "light".into(),
    })
}

fn worker_dreaming_deep(conn: &rusqlite::Connection) -> Result<WorkerDreamReport, String> {
    let tiers: &[(&str, i64)] = &[
        ("conversation", 30),
        ("daily", 30),
        ("note", 60),
        ("script", 60),
        ("learning", 90),
    ];
    let mut total_expired = 0u64;
    for (kind, days) in tiers {
        let now = chrono::Utc::now().to_rfc3339();
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(*days)).to_rfc3339();
        let n = conn.execute(
            "UPDATE mem_items SET importance = 0.0, updated_at = ?1 \
             WHERE kind = ?2 AND access_count = 0 AND created_at < ?3 \
             AND importance > 0.0 AND importance < 4.0",
            rusqlite::params![&now, kind, &cutoff],
        ).map_err(|e| e.to_string())? as u64;
        total_expired += n;
    }
    Ok(WorkerDreamReport {
        considered: total_expired,
        promoted: 0,
        dropped: total_expired,
        phase: "deep".into(),
    })
}

#[derive(Debug, Default, serde::Serialize)]
struct WorkerLibrarianReport {
    skill_folders_registered: usize,
    files_re_indexed: usize,
    files_unchanged: usize,
    source_missing: usize,
    source_recovered: usize,
    errors: Vec<String>,
}

fn worker_librarian_once(
    conn: &rusqlite::Connection,
    skills_root: &std::path::Path,
) -> Result<WorkerLibrarianReport, String> {
    let mut report = WorkerLibrarianReport::default();
    librarian_scan_skill_folders(conn, skills_root, &mut report);
    librarian_sync_files(conn, &mut report)?;
    Ok(report)
}

fn librarian_scan_skill_folders(
    conn: &rusqlite::Connection,
    skills_root: &std::path::Path,
    report: &mut WorkerLibrarianReport,
) {
    if !skills_root.is_dir() { return }
    let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT source_path FROM mem_items WHERE source_path LIKE 'file:%SKILL.md'"
    ) {
        if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
            for row in rows.flatten() { known.insert(row); }
        }
    }

    let entries = match std::fs::read_dir(skills_root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let folder = entry.path();
        if !folder.is_dir() { continue; }
        let skill_md = folder.join("SKILL.md");
        if !skill_md.is_file() { continue; }
        let source = format!("file:{}", skill_md.display());
        if known.contains(&source) { continue; }

        let body = match std::fs::read_to_string(&skill_md) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let title = title_from_skill_md(&body)
            .unwrap_or_else(|| folder.file_name().map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "skill".into()));
        let area = area_from_skill_md(&body);

        let id = build_skill_id(&title);
        let now = chrono::Utc::now().to_rfc3339();
        let tags_json = serde_json::to_string(&vec!["folder-skill", "auto-registered"]).unwrap_or("[]".into());
        let result = conn.execute(
            r#"INSERT OR IGNORE INTO mem_items
               (id, uid, kind, scope, title, summary, area, tags_json,
                body_inline, source, importance, created_at, updated_at,
                source_path)
               VALUES (?1, ?2, 'skill', 'global', ?3, NULL, ?4, ?5,
                       ?6, ?7, 4.0, ?8, ?8, ?7)"#,
            rusqlite::params![
                &id, &uuid::Uuid::new_v4().to_string(),
                &title, &area, &tags_json, &body, &source, &now,
            ],
        );
        match result {
            Ok(_n) => {
                let _ = conn.execute(
                    "INSERT OR REPLACE INTO mem_fts (mem_id, title, summary, body, tags) \
                     VALUES (?1, ?2, '', ?3, ?4)",
                    rusqlite::params![&id, &title, &body, &tags_json],
                );
                report.skill_folders_registered += 1;
            }
            Err(e) => report.errors.push(format!("register skill {}: {}", source, e)),
        }
    }
}

fn build_skill_id(title: &str) -> String {
    let slug: String = title.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let trimmed = slug.trim_matches('_');
    let short = if trimmed.len() > 40 { &trimmed[..40] } else { trimmed };
    format!("skill_{}", short)
}

fn title_from_skill_md(body: &str) -> Option<String> {
    if let Some(line) = body.lines().take(20).find(|l| l.trim_start().starts_with("title:")) {
        let v = line.trim_start().trim_start_matches("title:").trim().trim_matches('"').to_string();
        if !v.is_empty() { return Some(v); }
    }
    body.lines().find(|l| l.trim_start().starts_with("# "))
        .map(|l| l.trim_start().trim_start_matches("# ").trim().to_string())
}

fn area_from_skill_md(body: &str) -> Option<String> {
    body.lines().take(20)
        .find(|l| l.trim_start().starts_with("area:"))
        .map(|l| l.trim_start().trim_start_matches("area:").trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
}

struct LibrarianTrackedItem {
    id: String,
    source_path: String,
    source_size: Option<i64>,
    source_mtime: Option<String>,
    tags_json: String,
}

fn librarian_sync_files(
    conn: &rusqlite::Connection,
    report: &mut WorkerLibrarianReport,
) -> Result<(), String> {
    let items: Vec<LibrarianTrackedItem> = {
        let mut stmt = conn.prepare(
            "SELECT id, source_path, source_size, source_mtime, tags_json \
             FROM mem_items WHERE source_path IS NOT NULL"
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt.query_map([], |row| {
            Ok(LibrarianTrackedItem {
                id: row.get(0)?,
                source_path: row.get(1)?,
                source_size: row.get(2)?,
                source_mtime: row.get(3)?,
                tags_json: row.get(4)?,
            })
        }).map_err(|e| format!("query: {}", e))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    for item in &items {
        match librarian_sync_one(conn, item) {
            Ok(action) => match action {
                LibrarianSyncAction::Unchanged => report.files_unchanged += 1,
                LibrarianSyncAction::ReIndexed => report.files_re_indexed += 1,
                LibrarianSyncAction::Missing => report.source_missing += 1,
                LibrarianSyncAction::Recovered => report.source_recovered += 1,
            },
            Err(e) => report.errors.push(format!("{}: {}", item.id, e)),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum LibrarianSyncAction {
    Unchanged,
    ReIndexed,
    Missing,
    Recovered,
}

fn librarian_sync_one(
    conn: &rusqlite::Connection,
    item: &LibrarianTrackedItem,
) -> Result<LibrarianSyncAction, String> {
    let raw_path = item.source_path.strip_prefix("file:").unwrap_or(&item.source_path);
    let path = std::path::Path::new(raw_path);
    let has_missing_tag = librarian_has_tag(&item.tags_json, "source-missing");

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            if !has_missing_tag {
                librarian_add_tag(conn, &item.id, "source-missing")?;
            }
            return Ok(LibrarianSyncAction::Missing);
        }
    };

    let current_size = meta.len() as i64;
    let current_mtime = meta.modified().ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    let size_match = item.source_size.map_or(false, |s| s == current_size);
    let mtime_match = match (&item.source_mtime, &current_mtime) {
        (Some(stored), Some(current)) => stored == current,
        _ => false,
    };
    if size_match && mtime_match && !has_missing_tag {
        return Ok(LibrarianSyncAction::Unchanged);
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {}", item.source_path, e))?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE mem_items SET body_inline = ?1, body_blob_path = NULL, \
         source_size = ?2, source_mtime = ?3, updated_at = ?4 \
         WHERE id = ?5",
        rusqlite::params![&content, current_size, &current_mtime, &now, &item.id],
    ).map_err(|e| format!("update: {}", e))?;
    conn.execute(
        "UPDATE mem_fts SET body = ?1 WHERE mem_id = ?2",
        rusqlite::params![&content, &item.id],
    ).map_err(|e| format!("update fts: {}", e))?;
    // Drop any stale embedding so the daemon's reindex_missing pass picks
    // it up on the next startup tick. Done via vec0 virtual table — the
    // sqlite-vec auto-extension was registered before Connection::open.
    let _ = conn.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1",
        rusqlite::params![&item.id],
    );

    if has_missing_tag {
        librarian_remove_tag(conn, &item.id, "source-missing")?;
        return Ok(LibrarianSyncAction::Recovered);
    }
    Ok(LibrarianSyncAction::ReIndexed)
}

fn librarian_has_tag(tags_json: &str, tag: &str) -> bool {
    serde_json::from_str::<Vec<String>>(tags_json)
        .unwrap_or_default()
        .iter()
        .any(|t| t == tag)
}

fn librarian_add_tag(conn: &rusqlite::Connection, item_id: &str, tag: &str) -> Result<(), String> {
    let current: String = conn.query_row(
        "SELECT tags_json FROM mem_items WHERE id = ?1",
        [item_id], |r| r.get(0),
    ).map_err(|e| format!("read tags: {}", e))?;
    let mut tags: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
    if !tags.iter().any(|t| t == tag) {
        tags.push(tag.to_string());
        let new_json = serde_json::to_string(&tags).map_err(|e| format!("serialize: {}", e))?;
        conn.execute(
            "UPDATE mem_items SET tags_json = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![&new_json, chrono::Utc::now().to_rfc3339(), item_id],
        ).map_err(|e| format!("update tags: {}", e))?;
    }
    Ok(())
}

fn librarian_remove_tag(conn: &rusqlite::Connection, item_id: &str, tag: &str) -> Result<(), String> {
    let current: String = conn.query_row(
        "SELECT tags_json FROM mem_items WHERE id = ?1",
        [item_id], |r| r.get(0),
    ).map_err(|e| format!("read tags: {}", e))?;
    let mut tags: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
    tags.retain(|t| t != tag);
    let new_json = serde_json::to_string(&tags).map_err(|e| format!("serialize: {}", e))?;
    conn.execute(
        "UPDATE mem_items SET tags_json = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![&new_json, chrono::Utc::now().to_rfc3339(), item_id],
    ).map_err(|e| format!("update tags: {}", e))?;
    Ok(())
}

