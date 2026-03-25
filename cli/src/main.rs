mod client;

use clap::{Parser, Subcommand};
use client::{discover_socket, SocketClient};

/// Read API config (host, port, token) from CodeTerm config.toml
fn read_api_config() -> (String, Option<String>) {
    let config_dir = dirs::config_dir().unwrap_or_default().join("codeterm");
    let config_path = config_dir.join("config.toml");
    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(val) = content.parse::<toml::Table>() {
            if let Some(api) = val.get("api").and_then(|v| v.as_table()) {
                let raw_host = api.get("host").and_then(|v| v.as_str()).unwrap_or("127.0.0.1");
                // 0.0.0.0 means "listen on all interfaces" — for connecting, use localhost
                let host = if raw_host == "0.0.0.0" { "127.0.0.1" } else { raw_host };
                let port = api.get("port").and_then(|v| v.as_integer()).unwrap_or(7685);
                let token = api.get("token").and_then(|v| v.as_str()).map(|s| s.to_string());
                return (format!("http://{}:{}", host, port), token);
            }
        }
    }
    ("http://127.0.0.1:7685".to_string(), None)
}

/// Build HTTP client with optional auth header
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
        /// Parent pane ID (conductor or worker)
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
    /// Read reports (for conductor panes)
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
        /// Role: Conductor, Worker, SubWorker
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
                                        "Conductor" => "★",
                                        "Worker" => "⚙",
                                        "SubWorker" => "◦",
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
                        .json(&serde_json::json!({"role": "Conductor"}))
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
                                Some("Conductor") => "Worker",
                                Some("Worker") => "SubWorker",
                                _ => { eprintln!("Parent must be Conductor or Worker"); std::process::exit(1); }
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
    };

    match result {
        Ok(value) => {
            if cli.json {
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
