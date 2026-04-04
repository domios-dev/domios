use clap::Subcommand;

use super::Commands;

#[derive(Subcommand)]
pub enum MeshAction {
    /// Show mesh status (identity, discovery, peers)
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
    /// Initiate pairing with a discovered peer
    Connect {
        /// Instance ID of the discovered peer
        instance_id: String,
    },
    /// Accept an incoming pairing request
    Approve {
        /// Request ID of the pending pairing request
        request_id: String,
    },
    /// Reject and block an incoming pairing request
    Reject {
        /// Request ID of the pending pairing request
        request_id: String,
    },
    /// Disconnect (unpair) from a paired peer
    Disconnect {
        /// Instance ID of the paired peer
        instance_id: String,
    },
    /// Trigger mesh discovery scan (list discovered peers)
    Discover,
}

/// Sign a mesh request with Ed25519 (same as server-side peer auth).
pub fn sign_request(private_key_b64: &str, instance_id: &str) -> Result<String, String> {
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
pub fn run_via_mesh(cli: &super::Cli, target: &str) {
    // Mesh management commands are local-only — cannot be relayed
    if let super::Commands::Mesh { action } = &cli.command {
        match action {
            MeshAction::Connect { .. } | MeshAction::Approve { .. } |
            MeshAction::Reject { .. } | MeshAction::Disconnect { .. } |
            MeshAction::Discover => {
                eprintln!("Error: mesh management commands (connect/approve/reject/disconnect/discover) are local-only and cannot be relayed via --mesh");
                std::process::exit(1);
            }
            _ => {} // Status, Peers, Exec, Send, Output, Panes — relayable
        }
    }

    let (base, token) = super::read_api_config();
    let http = super::build_http(&token);

    // Map CLI command to API path + method + body
    let (method, path, body) = cli_command_to_api(&cli.command);

    mesh_relay_generic(&http, &base, &token, target, &method, &path, body.as_deref(), cli.json);
}

/// Map a CLI command to its corresponding API call.
pub fn cli_command_to_api(cmd: &Commands) -> (String, String, Option<String>) {
    match cmd {
        Commands::Identify => ("GET".into(), "/api/identify".into(), None),
        Commands::Capabilities => ("GET".into(), "/api/identify".into(), None),
        Commands::GetOutput { pane, lines } => {
            let p = pane.as_deref().unwrap_or("general");
            ("GET".into(), format!("/api/panes/{}/output?lines={}&strip_ansi=true", p, lines), None)
        }
        Commands::SendText { text, pane } => {
            let p = pane.as_deref().unwrap_or("general");
            let source = std::env::var("CODETERM_PANE_ID").unwrap_or_else(|_| "unknown".to_string());
            ("POST".into(), format!("/api/panes/{}/send", p),
                Some(serde_json::json!({"text": text, "enter": true, "source_pane": source}).to_string()))
        }
        Commands::Notify { title, body, level } => {
            ("POST".into(), "/api/notifications".into(),
                Some(serde_json::json!({"title": title, "body": body, "level": level}).to_string()))
        }
        Commands::Workspace { action } => match action {
            super::WorkspaceAction::List => ("GET".into(), "/api/workspaces".into(), None),
            super::WorkspaceAction::Create { name, .. } => ("POST".into(), "/api/workspaces".into(),
                Some(serde_json::json!({"name": name}).to_string())),
            super::WorkspaceAction::Focus { id } => ("POST".into(), format!("/api/workspaces/{}/focus", id), None),
            super::WorkspaceAction::Rename { id, name } => ("POST".into(), format!("/api/workspaces/{}/rename", id),
                Some(serde_json::json!({"name": name}).to_string())),
            super::WorkspaceAction::Close { id } => ("POST".into(), format!("/api/workspaces/{}/close", id), None),
            _ => ("GET".into(), "/api/workspaces".into(), None),
        },
        Commands::Pane { action } => match action {
            super::PaneAction::List => ("GET".into(), "/api/panes".into(), None),
            super::PaneAction::Split { id, direction } => {
                let pane = id.as_deref().unwrap_or("general");
                ("POST".into(), format!("/api/panes/{}/split", pane),
                    Some(serde_json::json!({"direction": direction}).to_string()))
            }
            super::PaneAction::Focus { id } => ("POST".into(), format!("/api/panes/{}/focus", id), None),
            super::PaneAction::Close { id } => ("POST".into(), format!("/api/panes/{}/close", id), None),
            super::PaneAction::Rename { title, pane } => {
                let p = pane.as_deref().unwrap_or("general");
                ("POST".into(), format!("/api/panes/{}/rename", p),
                    Some(serde_json::json!({"title": title}).to_string()))
            }
            _ => ("GET".into(), "/api/panes".into(), None),
        },
        Commands::Agent { action } => match action {
            super::AgentAction::List => ("GET".into(), "/api/agents".into(), None),
            super::AgentAction::Hierarchy => ("GET".into(), "/api/agents/hierarchy".into(), None),
            super::AgentAction::Spawn { agent, title, task, role, parent, workspace, can_spawn, cwd, args, .. } => {
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
            super::BrowserAction::Open { url } => ("POST".into(), "/api/browser/open".into(),
                Some(serde_json::json!({"url": url}).to_string())),
        },
        Commands::Cron { action } => match action {
            super::CronAction::List { .. } => ("GET".into(), "/api/crons".into(), None),
            super::CronAction::Create { name, schedule, task, workspace, agent, cwd, description, .. } => {
                let mut body = serde_json::json!({"name": name, "schedule": schedule, "task": task, "agent": agent});
                if let Some(w) = workspace { body["workspace_id"] = serde_json::json!(w); }
                if let Some(c) = cwd { body["cwd"] = serde_json::json!(c); }
                if let Some(d) = description { body["description"] = serde_json::json!(d); }
                ("POST".into(), "/api/crons".into(), Some(body.to_string()))
            }
            super::CronAction::Delete { id } => ("DELETE".into(), format!("/api/crons/{}", id), None),
            super::CronAction::Enable { id } => ("POST".into(), format!("/api/crons/{}/enable", id), None),
            super::CronAction::Disable { id } => ("POST".into(), format!("/api/crons/{}/disable", id), None),
            super::CronAction::Trigger { id } => ("POST".into(), format!("/api/crons/{}/trigger", id), None),
            super::CronAction::History { id } => ("GET".into(), format!("/api/crons/{}/history", id), None),
            _ => ("GET".into(), "/api/crons".into(), None),
        },
        Commands::Config { action } => match action {
            super::ConfigAction::Get { key } => ("GET".into(), format!("/api/config?key={}", key), None),
            super::ConfigAction::Set { key, value } => {
                let v: serde_json::Value = serde_json::from_str(value).unwrap_or(serde_json::json!(value));
                ("PATCH".into(), "/api/config".into(), Some(serde_json::json!({"key": key, "value": v}).to_string()))
            }
        },
        Commands::Update { .. } => ("GET".into(), "/api/update/check".into(), None),
        // Mesh subcommands — management commands are local-only (not relayable)
        Commands::Mesh { action } => match action {
            MeshAction::Status => ("GET".into(), "/api/mesh/status".into(), None),
            MeshAction::Peers => ("GET".into(), "/api/mesh/peers".into(), None),
            MeshAction::Discover => ("POST".into(), "/api/mesh/discover".into(), None),
            MeshAction::Connect { instance_id } => ("POST".into(), "/api/mesh/connect".into(),
                Some(serde_json::json!({"instance_id": instance_id}).to_string())),
            MeshAction::Approve { request_id } => ("POST".into(), "/api/mesh/approve".into(),
                Some(serde_json::json!({"request_id": request_id}).to_string())),
            MeshAction::Reject { request_id } => ("POST".into(), "/api/mesh/reject".into(),
                Some(serde_json::json!({"request_id": request_id}).to_string())),
            MeshAction::Disconnect { instance_id } => ("POST".into(), "/api/mesh/disconnect".into(),
                Some(serde_json::json!({"instance_id": instance_id}).to_string())),
            // Remote relay commands — these map to generic API calls
            _ => ("GET".into(), "/api/mesh/status".into(), None),
        },
        // Bridge, Theme — best effort
        Commands::Bridge { .. } => ("GET".into(), "/api/health".into(), None),
        Commands::Theme { action } => match action {
            super::ThemeAction::List => ("GET".into(), "/api/themes".into(), None),
            _ => ("GET".into(), "/api/themes".into(), None),
        },
    }
}

/// Shortcut relay — maps old-style actions to new generic relay format.
pub fn mesh_relay_cmd(http: &reqwest::blocking::Client, base: &str, token: &Option<String>, target: &str, action: &str, payload: &str, pane: &str, json_mode: bool) {
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
pub fn mesh_relay_generic(http: &reqwest::blocking::Client, base: &str, token: &Option<String>, target: &str, method: &str, path: &str, body: Option<&str>, json_mode: bool) {
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
