mod client;

use clap::{Parser, Subcommand};
use client::{discover_socket, SocketClient};

/// Determine the CodeTerm data directory.
/// Dev instances use `~/.codeterm-dev/`, prod uses `~/.codeterm/`.
/// Detection: if CODETERM_SOCKET_PATH contains "codeterm-dev", use dev dir.
fn data_dir_name() -> &'static str {
    if let Ok(sp) = std::env::var("CODETERM_SOCKET_PATH") {
        if sp.contains("codeterm-dev") {
            return ".codeterm-dev";
        }
    }
    ".codeterm"
}

/// Read API config (host, port, token).
/// Priority: $CODETERM_API_PORT env var (set by CodeTerm PTY) > config.toml.
/// This ensures CLI works in both dev (7686) and prod (7685) when called from inside CodeTerm.
fn read_api_config() -> (String, Option<String>) {
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

    // Sign with Ed25519 identity key
    let identity_path = config_dir.join("mesh").join("identity.json");
    let token = std::fs::read_to_string(&identity_path).ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .and_then(|id| {
            let pk = id.get("private_key")?.as_str()?;
            let iid = id.get("instance_id")?.as_str()?;
            sign_request(pk, iid).ok()
        });

    (format!("http://{}:{}", host, port), token)
}

/// Sign a mesh request with Ed25519 (same as server-side crown::sign_request).
fn sign_request(private_key_b64: &str, instance_id: &str) -> Result<String, String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;

    let key_bytes = b64.decode(private_key_b64).map_err(|e| e.to_string())?;
    if key_bytes.len() != 32 { return Err("Invalid key length".to_string()); }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&key_bytes);
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&arr);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let payload = serde_json::json!({
        "from_instance_id": instance_id,
        "timestamp": now,
        "nonce": uuid::Uuid::new_v4().to_string(),
    });
    let payload_bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    let payload_b64 = b64.encode(&payload_bytes);

    use ed25519_dalek::Signer;
    let signature = signing_key.sign(payload_b64.as_bytes());
    let sig_b64 = b64.encode(signature.to_bytes());
    Ok(format!("{}.{}", payload_b64, sig_b64))
}

/// Execute a CLI command on a remote mesh peer via relay.
/// Reconstructs the command as an API call and sends through relay proxy.
fn run_via_mesh(cli: &Cli, target: &str) {
    let (base, token) = read_api_config();
    let http = build_http(&token);

    // Map CLI command to API path + method + body
    let (method, path, body) = cli_command_to_api(&cli.command);

    mesh_relay_generic(&http, &base, &token, target, &method, &path, body.as_deref(), cli.json);
}

/// Map a CLI command to its corresponding API call.
fn cli_command_to_api(cmd: &Commands) -> (String, String, Option<String>) {
    match cmd {
        Commands::Identify => ("GET".into(), "/api/identify".into(), None),
        Commands::Capabilities => ("GET".into(), "/api/identify".into(), None),
        Commands::GetOutput { pane, lines } => {
            let p = pane.as_deref().unwrap_or("general");
            ("GET".into(), format!("/api/panes/{}/output?lines={}", p, lines), None)
        }
        Commands::SendText { text, pane } => {
            let p = pane.as_deref().unwrap_or("general");
            ("POST".into(), format!("/api/panes/{}/send", p),
                Some(serde_json::json!({"text": text, "enter": true}).to_string()))
        }
        Commands::Notify { title, body, level } => {
            ("POST".into(), "/api/notifications".into(),
                Some(serde_json::json!({"title": title, "body": body, "level": level}).to_string()))
        }
        Commands::Workspace { action } => match action {
            WorkspaceAction::List => ("GET".into(), "/api/workspaces".into(), None),
            WorkspaceAction::Create { name, .. } => ("POST".into(), "/api/workspaces".into(),
                Some(serde_json::json!({"name": name}).to_string())),
            WorkspaceAction::Focus { id } => ("POST".into(), format!("/api/workspaces/{}/focus", id), None),
            WorkspaceAction::Rename { id, name } => ("POST".into(), format!("/api/workspaces/{}/rename", id),
                Some(serde_json::json!({"name": name}).to_string())),
            WorkspaceAction::Close { id } => ("POST".into(), format!("/api/workspaces/{}/close", id), None),
            _ => ("GET".into(), "/api/workspaces".into(), None),
        },
        Commands::Pane { action } => match action {
            PaneAction::List => ("GET".into(), "/api/panes".into(), None),
            PaneAction::Split { id, direction } => {
                let pane = id.as_deref().unwrap_or("general");
                ("POST".into(), format!("/api/panes/{}/split", pane),
                    Some(serde_json::json!({"direction": direction}).to_string()))
            }
            PaneAction::Focus { id } => ("POST".into(), format!("/api/panes/{}/focus", id), None),
            PaneAction::Close { id } => ("POST".into(), format!("/api/panes/{}/close", id), None),
            PaneAction::Rename { title, pane } => {
                let p = pane.as_deref().unwrap_or("general");
                ("POST".into(), format!("/api/panes/{}/rename", p),
                    Some(serde_json::json!({"title": title}).to_string()))
            }
            _ => ("GET".into(), "/api/panes".into(), None),
        },
        Commands::Agent { action } => match action {
            AgentAction::List => ("GET".into(), "/api/agents".into(), None),
            AgentAction::Hierarchy => ("GET".into(), "/api/agents/hierarchy".into(), None),
            AgentAction::Spawn { agent, title, task, role, parent, workspace, can_spawn, cwd, args, .. } => {
                let agent_name = agent.as_str();
                let mut body = serde_json::json!({
                    "agent": agent_name,
                    "title": title,
                    "task": task,
                });
                if let Some(r) = role { body["role"] = serde_json::json!(r); }
                if let Some(p) = parent { body["parent_pane_id"] = serde_json::json!(p); }
                if let Some(w) = workspace { body["workspace_id"] = serde_json::json!(w); }
                if *can_spawn { body["can_spawn"] = serde_json::json!(true); }
                if let Some(c) = cwd { body["cwd"] = serde_json::json!(c); }
                if let Some(a) = args { if !a.is_empty() { body["args"] = serde_json::json!(a); } }
                ("POST".into(), "/api/agents/spawn".into(), Some(body.to_string()))
            }
            _ => ("GET".into(), "/api/agents".into(), None),
        },
        Commands::Notifications { unread, .. } => {
            let qs = if *unread { "?unread=true" } else { "" };
            ("GET".into(), format!("/api/notifications{}", qs), None)
        }
        Commands::Browser { action } => match action {
            BrowserAction::Open { url } => ("POST".into(), "/api/browser/open".into(),
                Some(serde_json::json!({"url": url}).to_string())),
        },
        Commands::Cron { action } => match action {
            CronAction::List { .. } => ("GET".into(), "/api/crons".into(), None),
            CronAction::Create { name, schedule, task, workspace, agent, cwd, description, .. } => {
                let mut body = serde_json::json!({"name": name, "schedule": schedule, "task": task, "agent": agent});
                if let Some(w) = workspace { body["workspace_id"] = serde_json::json!(w); }
                if let Some(c) = cwd { body["cwd"] = serde_json::json!(c); }
                if let Some(d) = description { body["description"] = serde_json::json!(d); }
                ("POST".into(), "/api/crons".into(), Some(body.to_string()))
            }
            CronAction::Delete { id } => ("DELETE".into(), format!("/api/crons/{}", id), None),
            CronAction::Enable { id } => ("POST".into(), format!("/api/crons/{}/enable", id), None),
            CronAction::Disable { id } => ("POST".into(), format!("/api/crons/{}/disable", id), None),
            CronAction::Trigger { id } => ("POST".into(), format!("/api/crons/{}/trigger", id), None),
            CronAction::History { id } => ("GET".into(), format!("/api/crons/{}/history", id), None),
            _ => ("GET".into(), "/api/crons".into(), None),
        },
        Commands::Config { action } => match action {
            ConfigAction::Get { key } => ("GET".into(), format!("/api/config?key={}", key), None),
            ConfigAction::Set { key, value } => {
                let v: serde_json::Value = serde_json::from_str(value).unwrap_or(serde_json::json!(value));
                ("PATCH".into(), "/api/config".into(), Some(serde_json::json!({"key": key, "value": v}).to_string()))
            }
        },
        Commands::Screenshot { .. } => ("GET".into(), "/api/screenshot".into(), None),
        Commands::Update { .. } => ("GET".into(), "/api/update/check".into(), None),
        // Mesh subcommands run locally (no recursion)
        Commands::Mesh { .. } => ("GET".into(), "/api/mesh/status".into(), None),
        // Bridge, Theme — best effort
        Commands::Bridge { .. } => ("GET".into(), "/api/health".into(), None),
        Commands::Theme { action } => match action {
            ThemeAction::List => ("GET".into(), "/api/themes".into(), None),
            _ => ("GET".into(), "/api/themes".into(), None),
        },
    }
}

/// Build HTTP client with auth header
fn build_http(token: &Option<String>) -> reqwest::blocking::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(t) = token {
        if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t)) {
            headers.insert(reqwest::header::AUTHORIZATION, val);
        }
    }
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

#[derive(Parser)]
#[command(name = "codeterm", version, about = "CodeTerm CLI — control your terminal multiplexer")]
struct Cli {
    /// Output raw JSON instead of human-readable text
    #[arg(long, global = true)]
    json: bool,

    /// Execute command on a remote mesh peer instead of locally
    #[arg(long, global = true)]
    mesh: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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

    /// Capture a screenshot of the CodeTerm window
    Screenshot {
        /// Save to file instead of printing base64 (e.g. screenshot.jpg)
        #[arg(long)]
        output: Option<String>,

        /// Output format: json (default, data URL), file (save to --output)
        #[arg(long, default_value = "json")]
        format: String,
    },
}

#[derive(Subcommand)]
enum WorkspaceAction {
    /// List all workspaces
    List,
    /// Print the current (focused) workspace ID
    Current,
    /// Create a new workspace
    Create {
        /// Workspace name
        name: String,
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
enum PaneAction {
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
    /// Split a pane (default: current pane)
    Split {
        /// Pane ID to split (default: current focused pane)
        #[arg(long, alias = "pane")]
        id: Option<String>,
        /// Split direction: right or down
        #[arg(long, default_value = "right")]
        direction: String,
    },
    /// Rename a pane tab (default: current pane)
    Rename {
        /// New title
        title: String,
        /// Pane ID to rename (default: current focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,
    },
    /// Get visible screen content of a pane (what the user sees)
    Screen {
        /// Pane ID (default: current focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,
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
enum BrowserAction {
    /// Open a URL in the canvas panel
    Open {
        /// URL to open
        url: String,
    },
}

#[derive(Subcommand)]
enum NotificationAction {
    /// Clear all notifications
    Clear,
}

#[derive(Subcommand)]
enum ThemeAction {
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
enum AgentAction {
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
    },
    /// Get detailed status of a pane
    Status {
        /// Pane ID (default: current focused pane)
        #[arg(long)]
        pane: Option<String>,
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
enum UpdateAction {
    /// Check for available updates
    Check {
        /// Force re-check (ignore cache)
        #[arg(long)]
        force: bool,
    },
    /// Show update command or open download page
    Install,
}

#[derive(Subcommand)]
enum BridgeAction {
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
enum ConfigAction {
    /// Get a config value (dot-notation, e.g. "integrations.telegram_enabled")
    Get { key: String },
    /// Set a config value (JSON-compatible: true, false, 42, "string")
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum CronAction {
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
        /// Cron schedule (5-field: "*/30 * * * *")
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
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Pane ID to receive reports from cron agent (default: $CODETERM_PANE_ID)
        #[arg(long, env = "CODETERM_PANE_ID")]
        creator_pane: Option<String>,
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

#[derive(Subcommand)]
enum MeshAction {
    /// Show mesh status (identity, crown, discovery)
    Status,
    /// List all peers (paired + discovered)
    Peers,
    /// Execute a command on a remote mesh peer
    Exec {
        /// Target peer name or instance ID
        target: String,
        /// Command to execute
        command: String,
        /// Target pane on remote instance (default: general)
        #[arg(long, default_value = "general")]
        pane: String,
    },
    /// Send text to a pane on a remote mesh peer
    Send {
        /// Target peer name or instance ID
        target: String,
        /// Text to send
        text: String,
        /// Target pane on remote instance (default: general)
        #[arg(long, default_value = "general")]
        pane: String,
    },
    /// Get output from a pane on a remote mesh peer
    Output {
        /// Target peer name or instance ID
        target: String,
        /// Target pane on remote instance (default: general)
        #[arg(long, default_value = "general")]
        pane: String,
    },
    /// List panes on a remote mesh peer
    Panes {
        /// Target peer name or instance ID
        target: String,
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
        run_via_mesh(&cli, mesh_target);
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
                    Ok(_) => Err(format!("Key not found: {}", key)),
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
                if json_output { println!("{}", serde_json::to_string_pretty(&value).unwrap()); }
                else { print_human(&value); }
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
            WorkspaceAction::Create { name } => {
                client.call("workspace.create", Some(serde_json::json!({ "name": name })))
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
            PaneAction::Split { id, direction } => {
                let pane_id = resolve_pane(&mut client, id);
                client.call(
                    "pane.split",
                    Some(serde_json::json!({ "pane_id": pane_id, "direction": direction })),
                )
            }
            PaneAction::Rename { pane, title } => {
                let pane_id = resolve_pane(&mut client, pane);
                client.call(
                    "pane.rename",
                    Some(serde_json::json!({ "pane_id": pane_id, "title": title })),
                )
            }
            PaneAction::Screen { pane } => {
                let pane_id = resolve_pane(&mut client, pane);
                let (api_url, api_token) = read_api_config();
                let http = build_http(&api_token);
                match http.get(format!("{}/api/panes/{}/screen", api_url, pane_id)).send() {
                    Ok(resp) => {
                        if let Ok(val) = resp.json::<serde_json::Value>() {
                            if let Some(screen) = val.get("screen").and_then(|v| v.as_str()) {
                                for line in screen.lines() {
                                    if !line.trim().is_empty() {
                                        println!("{}", line);
                                    }
                                }
                                return;
                            }
                        }
                        eprintln!("No screen data");
                        std::process::exit(1);
                    }
                    Err(e) => { eprintln!("Screen request failed: {}", e); std::process::exit(1); }
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
            // Approximate: ~80 bytes per line
            let max_bytes = (lines as u64) * 80;
            let result = client.call(
                "pane.get_output",
                Some(serde_json::json!({ "pane_id": pane_id, "max_bytes": max_bytes })),
            );
            match result {
                Ok(value) => {
                    if let Some(output) = value.get("output").and_then(|v| v.as_str()) {
                        print!("{}", output);
                    } else if cli.json {
                        println!("{}", serde_json::to_string_pretty(&value).unwrap());
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
                AgentAction::Reports { pane, clear } => {
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
                                for r in arr {
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
                            }
                            return;
                        }
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
                                    let status = if enabled { "enabled" } else { "disabled" };
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
                CronAction::Create { name, schedule, task, workspace, agent, cwd, description, creator_pane } => {
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
                    if let Some(c) = cwd { params["cwd"] = serde_json::json!(c); }
                    if let Some(d) = description { params["description"] = serde_json::json!(d); }
                    if let Some(cp) = creator_pane { params["creator_pane_id"] = serde_json::json!(cp); }
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
                CronAction::Edit { id, name, schedule, task, cwd, description } => {
                    let mut params = serde_json::json!({"id": id});
                    if let Some(n) = name { params["name"] = serde_json::json!(n); }
                    if let Some(s) = schedule { params["schedule"] = serde_json::json!(s); }
                    if let Some(t) = task { params["task"] = serde_json::json!(t); }
                    if let Some(c) = cwd { params["cwd"] = serde_json::json!(c); }
                    if let Some(d) = description { params["description"] = serde_json::json!(d); }
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

        Commands::Update { action } => {
            let (base_url, token) = read_api_config();
            let http = build_http(&token);

            match action.unwrap_or(UpdateAction::Check { force: false }) {
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
                UpdateAction::Install => {
                    match http.post(format!("{}/api/update/install", base_url)).send() {
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
            }
        }

        // Config is handled before socket connect (uses HTTP API)
        Commands::Config { .. } => unreachable!(),

        Commands::Screenshot { output, format } => {
            let base = "http://127.0.0.1:7685";
            let http = reqwest::blocking::Client::new();

            if format == "file" || output.is_some() {
                // Save JPEG to file
                let path = output.unwrap_or_else(|| "screenshot.jpg".to_string());
                match http.get(format!("{}/api/screenshot?format=jpeg", base)).send() {
                    Ok(resp) => {
                        if resp.status().is_success() {
                            let bytes = resp.bytes().unwrap_or_default();
                            if let Err(e) = std::fs::write(&path, &bytes) {
                                eprintln!("Error writing file: {}", e);
                                std::process::exit(1);
                            }
                            println!("{} ({}KB)", path, bytes.len() / 1024);
                        } else {
                            eprintln!("Error: {}", resp.status());
                            std::process::exit(1);
                        }
                    }
                    Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                }
            } else {
                // JSON with data URL
                match http.get(format!("{}/api/screenshot", base)).send() {
                    Ok(resp) => {
                        let val: serde_json::Value = resp.json().unwrap_or_default();
                        if cli.json {
                            println!("{}", serde_json::to_string_pretty(&val).unwrap());
                        } else {
                            println!("{}x{} ({}KB)",
                                val.get("width").and_then(|v| v.as_u64()).unwrap_or(0),
                                val.get("height").and_then(|v| v.as_u64()).unwrap_or(0),
                                val.get("size_kb").and_then(|v| v.as_u64()).unwrap_or(0),
                            );
                            if let Some(data) = val.get("screenshot").and_then(|v| v.as_str()) {
                                println!("{}", data);
                            }
                        }
                    }
                    Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                }
            }
            return;
        }

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
                                let crown = val.get("is_crown").and_then(|v| v.as_bool()).unwrap_or(false);
                                let paired = val.get("paired_count").and_then(|v| v.as_u64()).unwrap_or(0);
                                let discovered = val.get("discovered_count").and_then(|v| v.as_u64()).unwrap_or(0);
                                println!("{} ({})", name, &id[..id.len().min(8)]);
                                println!("Crown: {}", if crown { "yes" } else { "no" });
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
                                        let trust = p.get("trust_level").and_then(|v| v.as_str()).unwrap_or("?");
                                        let ip = p.get("addresses").and_then(|v| v.as_array())
                                            .and_then(|a| a.first())
                                            .and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("?");
                                        let dot = if online { "●" } else { "○" };
                                        println!("  {} {} ({}) [{}] {}", dot, name, ip, trust,
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
                    mesh_relay_cmd(&http, &base, &token, &target, action, &command, &pane, cli.json);
                    return;
                }
                MeshAction::Send { target, text, pane } => {
                    mesh_relay_cmd(&http, &base, &token, &target, "send", &text, &pane, cli.json);
                    return;
                }
                MeshAction::Output { target, pane } => {
                    mesh_relay_cmd(&http, &base, &token, &target, "output", "", &pane, cli.json);
                    return;
                }
                MeshAction::Panes { target } => {
                    mesh_relay_cmd(&http, &base, &token, &target, "panes", "", "", cli.json);
                    return;
                }
            }
        }
    };

    handle_result(cli.json, result);
}

/// Shortcut relay — maps old-style actions to new generic relay format.
fn mesh_relay_cmd(http: &reqwest::blocking::Client, base: &str, token: &Option<String>, target: &str, action: &str, payload: &str, pane: &str, json_mode: bool) {
    let (method, path, body) = match action {
        "exec" | "send" => ("POST", format!("/api/panes/{}/send", if pane.is_empty() { "general" } else { pane }),
            serde_json::json!({"text": payload, "enter": true})),
        "output" => ("GET", format!("/api/panes/{}/output", if pane.is_empty() { "general" } else { pane }),
            serde_json::json!(null)),
        "panes" => ("GET", "/api/panes".to_string(), serde_json::json!(null)),
        "status" => ("GET", "/api/mesh/status".to_string(), serde_json::json!(null)),
        _ => { eprintln!("Unknown action: {}", action); std::process::exit(1); }
    };
    let body_str = if body.is_null() { None } else { Some(body.to_string()) };
    mesh_relay_generic(http, base, token, target, method, &path, body_str.as_deref(), json_mode);
}

/// Generic relay — send any API call through mesh to a remote peer.
fn mesh_relay_generic(http: &reqwest::blocking::Client, base: &str, token: &Option<String>, target: &str, method: &str, path: &str, body: Option<&str>, json_mode: bool) {
    // Resolve peer address
    let peers_resp = http.get(format!("{}/api/mesh/peers", base))
        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
        .send().ok().and_then(|r| r.json::<serde_json::Value>().ok());
    let peer = peers_resp.as_ref()
        .and_then(|v| v.get("paired")).and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|p| {
            p.get("instance_name").and_then(|v| v.as_str()) == Some(target)
            || p.get("instance_id").and_then(|v| v.as_str()) == Some(target)
        }));
    let Some(peer) = peer else {
        eprintln!("Peer '{}' not found", target);
        std::process::exit(1);
    };
    let ip = peer.get("addresses").and_then(|v| v.as_array())
        .and_then(|a| a.first()).and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("127.0.0.1");
    let port = peer.get("api_port").and_then(|v| v.as_u64()).unwrap_or(7685);
    let peer_url = format!("http://{}:{}", ip, port);

    // Sign with our identity for auth
    let signed_token = token.as_deref().unwrap_or("");

    let relay = serde_json::json!({
        "method": method,
        "path": path,
        "body": body.and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok()).unwrap_or(serde_json::json!(null)),
    });

    match http.post(format!("{}/api/mesh/relay", peer_url))
        .header("Authorization", format!("Bearer {}", signed_token))
        .json(&relay).send() {
        Ok(resp) => {
            let val: serde_json::Value = resp.json().unwrap_or_default();
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&val).unwrap());
            } else if let Some(data) = val.get("data") {
                if let Some(text) = data.get("text").and_then(|v| v.as_str()) {
                    print!("{}", text); // raw output for output command
                } else {
                    println!("{}", serde_json::to_string_pretty(data).unwrap());
                }
            } else if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
                eprintln!("Error: {}", err);
            }
        }
        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
    }
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
