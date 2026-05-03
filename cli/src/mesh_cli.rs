use clap::Subcommand;

use super::{Commands, UpdateAction};

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
    /// Start broadcasting for the configured duration
    Broadcast,
    /// Stop broadcasting immediately
    #[clap(name = "stop-broadcast")]
    StopBroadcast,
    /// Re-detect local addresses (Tailscale + LAN) and announce them to all
    /// paired peers. Use after a network swap so peers update their cache
    /// without restarting CodeTerm.
    Refresh,
    /// Push our current address list to paired peers without re-detecting
    /// locally. Use --to to limit to one peer.
    Announce {
        /// Optional target peer (instance_id or name). Omit for all peers.
        #[arg(long)]
        to: Option<String>,
    },
    /// Per-peer address management.
    Peer {
        #[command(subcommand)]
        action: MeshPeerAction,
    },
}

#[derive(Subcommand)]
pub enum MeshPeerAction {
    /// Manually override our cached address for a paired peer. Useful when
    /// auto-detection is wrong or you want to point at a specific interface.
    /// Format: ip[:port]. Examples:
    ///   codeterm mesh peer set-addr BLACKSTAR 100.84.159.37
    ///   codeterm mesh peer set-addr BLACKSTAR 192.168.1.42:7685
    #[clap(name = "set-addr")]
    SetAddr {
        /// Target peer (instance_id or name)
        peer: String,
        /// New address as ip[:port]
        addr: String,
    },
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
            MeshAction::Discover | MeshAction::Broadcast | MeshAction::StopBroadcast |
            MeshAction::Peer { .. } => {
                eprintln!("Error: mesh management commands are local-only and cannot be relayed via --mesh");
                std::process::exit(1);
            }
            _ => {} // Status, Peers, Exec, Send, Output, Panes, Refresh, Announce — relayable
        }
    }

    let (base, token) = super::read_api_config();
    let http = super::build_http(&token);

    // Update --apply over mesh needs check-then-install in sequence.
    if let Commands::Update { action, check, apply, channel, manifest_url } = &cli.command {
        let resolved = resolve_update_action(action.as_ref(), *check, *apply, channel.clone(), manifest_url.clone());
        if let UpdateAction::Install { auto: true, manifest_url } = &resolved {
            run_update_apply_via_mesh(&http, &base, &token, target, manifest_url.as_deref(), cli.json);
            return;
        }
    }

    // Map CLI command to API path + method + body
    let (method, path, body) = cli_command_to_api(&cli.command);

    mesh_relay_generic(&http, &base, &token, target, &method, &path, body.as_deref(), cli.json);
}

/// Resolve update flag shortcuts (--check, --apply, --channel) to a concrete UpdateAction.
/// Same logic as the local CLI dispatch in main.rs.
fn resolve_update_action(
    action: Option<&UpdateAction>,
    check: bool,
    apply: bool,
    channel: Option<String>,
    manifest_url: Option<String>,
) -> UpdateAction {
    if let Some(ch) = channel {
        UpdateAction::Channel {
            channel: if ch.is_empty() { None } else { Some(ch) },
        }
    } else if apply {
        UpdateAction::Install { auto: true, manifest_url }
    } else if check {
        UpdateAction::Check { force: false }
    } else {
        match action {
            Some(UpdateAction::Check { force }) => UpdateAction::Check { force: *force },
            Some(UpdateAction::Install { auto, manifest_url }) => UpdateAction::Install {
                auto: *auto,
                manifest_url: manifest_url.clone(),
            },
            Some(UpdateAction::Channel { channel }) => UpdateAction::Channel { channel: channel.clone() },
            None => UpdateAction::Check { force: false },
        }
    }
}

/// Two-step update apply over mesh: force-check (populates peer's last_check
/// cache), then install if an update is available. Prints a one-line status
/// per step. The peer's install endpoint requires a prior check call.
fn run_update_apply_via_mesh(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
    manifest_url: Option<&str>,
    json_mode: bool,
) {
    let Some(peer_url) = resolve_peer_url(http, base, token, target) else {
        eprintln!("Peer '{}' not found", target);
        std::process::exit(1);
    };

    let signed = token.as_deref().unwrap_or("");

    // Step 1: force-check on peer.
    let check_resp = relay_call(http, &peer_url, signed, "GET", "/api/update/check?force=true", None);
    let check_data = match check_resp {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[{}] check failed: {}", target, e);
            std::process::exit(1);
        }
    };
    let status = check_data.get("status").and_then(|v| v.as_u64()).unwrap_or(0);
    if status == 404 {
        eprintln!(
            "Peer '{}' is running an older CodeTerm without remote-update support.\n\
             Bootstrap manually one last time, then this command will work going forward.",
            target
        );
        std::process::exit(2);
    }
    if !check_data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        let err = check_data.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
        eprintln!("[{}] check failed: {} (HTTP {})", target, err, status);
        std::process::exit(1);
    }
    let info = check_data.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let current = info.get("current_version").and_then(|v| v.as_str()).unwrap_or("?");
    let latest = info.get("latest_version").and_then(|v| v.as_str()).unwrap_or("?");
    let available = info.get("update_available").and_then(|v| v.as_bool()).unwrap_or(false);

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "step": "check",
            "peer": target,
            "data": info,
        })).unwrap_or_default());
    } else {
        println!(
            "[{}] checking…  current v{}, latest v{} — {}",
            target, current, latest,
            if available { "update available" } else { "up to date" }
        );
    }

    if !available {
        return;
    }

    // Step 2: install on peer.
    let mut install_path = String::from("/api/update/install?method=auto");
    if let Some(m) = manifest_url {
        install_path.push_str(&format!("&manifest_url={}", urlencoding::encode(m)));
    }
    let install_resp = relay_call(http, &peer_url, signed, "POST", &install_path, None);
    let install_data = match install_resp {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[{}] install relay failed: {}", target, e);
            std::process::exit(1);
        }
    };
    let status = install_data.get("status").and_then(|v| v.as_u64()).unwrap_or(0);
    if status == 403 {
        let err = install_data.get("data").and_then(|d| d.get("error")).and_then(|v| v.as_str())
            .or_else(|| install_data.get("error").and_then(|v| v.as_str()))
            .unwrap_or("forbidden");
        eprintln!("[{}] install rejected: {}", target, err);
        std::process::exit(3);
    }
    if status == 404 {
        eprintln!(
            "Peer '{}' is running an older CodeTerm without remote-update support.\n\
             Bootstrap manually one last time, then this command will work going forward.",
            target
        );
        std::process::exit(2);
    }
    if !install_data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        let err = install_data.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
        eprintln!("[{}] install failed: {} (HTTP {})", target, err, status);
        std::process::exit(1);
    }
    let inner = install_data.get("data").cloned().unwrap_or(serde_json::Value::Null);
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "step": "install",
            "peer": target,
            "data": inner,
        })).unwrap_or_default());
    } else {
        let s = inner.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        let msg = inner.get("message").and_then(|v| v.as_str()).unwrap_or("");
        println!("[{}] applying…  status={} {}", target, s, msg);
    }
}

/// Resolve a peer's API URL by name or instance_id via the local /api/mesh/peers.
fn resolve_peer_url(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
) -> Option<String> {
    let peers_resp = http.get(format!("{}/api/mesh/peers", base))
        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
        .send().ok().and_then(|r| r.json::<serde_json::Value>().ok())?;
    let paired = peers_resp.get("paired").and_then(|v| v.as_array())?;
    let peer = paired.iter().find(|p| {
        p.get("instance_name").and_then(|v| v.as_str()) == Some(target)
            || p.get("instance_id").and_then(|v| v.as_str()) == Some(target)
    })?;
    let ip = peer.get("addresses").and_then(|v| v.as_array())
        .and_then(|a| a.first()).and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("127.0.0.1");
    let port = peer.get("api_port").and_then(|v| v.as_u64()).unwrap_or(7685);
    Some(format!("http://{}:{}", ip, port))
}

/// Send a relay envelope to a peer's /api/mesh/relay and return the parsed JSON response.
fn relay_call(
    http: &reqwest::blocking::Client,
    peer_url: &str,
    signed_token: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<serde_json::Value, String> {
    let relay = serde_json::json!({
        "method": method,
        "path": path,
        "body": body.and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok()).unwrap_or(serde_json::json!(null)),
    });
    let resp = http.post(format!("{}/api/mesh/relay", peer_url))
        .header("Authorization", format!("Bearer {}", signed_token))
        .json(&relay).send()
        .map_err(|e| e.to_string())?;
    resp.json::<serde_json::Value>().map_err(|e| e.to_string())
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
            super::PaneAction::Current { .. } => ("GET".into(), "/api/panes".into(), None),
            super::PaneAction::Focus { id } => ("POST".into(), format!("/api/panes/{}/focus", id), None),
            super::PaneAction::Close { id } => ("POST".into(), format!("/api/panes/{}/close", id), None),
            super::PaneAction::Rename { title, pane } => {
                let p = pane.as_deref().unwrap_or("general");
                ("POST".into(), format!("/api/panes/{}/rename", p),
                    Some(serde_json::json!({"title": title}).to_string()))
            }
            super::PaneAction::GetText { pane, formatted } => {
                let p = pane.as_deref().unwrap_or("general");
                ("GET".into(), format!("/api/panes/{}/text?formatted={}", p, formatted), None)
            }
            super::PaneAction::Exec { command, pane, timeout } => {
                let p = pane.as_deref().unwrap_or("general");
                ("POST".into(), format!("/api/panes/{}/exec", p),
                    Some(serde_json::json!({
                        "command": command,
                        "timeout_ms": timeout,
                        "strip_ansi": true,
                    }).to_string()))
            }
            super::PaneAction::Status { pane } => {
                let p = pane.as_deref().unwrap_or("general");
                ("GET".into(), format!("/api/panes/{}/status", p), None)
            }
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
            super::CronAction::Create { name, schedule, task, workspace, agent, model, cwd, description, .. } => {
                let mut body = serde_json::json!({"name": name, "schedule": schedule, "task": task, "agent": agent});
                if let Some(m) = model { body["model"] = serde_json::json!(m); }
                if let Some(w) = workspace { body["workspace_id"] = serde_json::json!(w); }
                if let Some(c) = cwd { body["cwd"] = serde_json::json!(c); }
                if let Some(d) = description { body["description"] = serde_json::json!(d); }
                ("POST".into(), "/api/crons".into(), Some(body.to_string()))
            }
            super::CronAction::Delete { id } => ("DELETE".into(), format!("/api/crons/{}", id), None),
            super::CronAction::Enable { id } => ("POST".into(), format!("/api/crons/{}/enable", id), None),
            super::CronAction::Disable { id } => ("POST".into(), format!("/api/crons/{}/disable", id), None),
            super::CronAction::Trigger { id } => ("POST".into(), format!("/api/crons/{}/trigger", id), None),
            super::CronAction::Cancel { id } => ("POST".into(), format!("/api/crons/{}/cancel", id), None),
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
        Commands::Update { action, check, apply, channel, manifest_url } => {
            let resolved = resolve_update_action(action.as_ref(), *check, *apply, channel.clone(), manifest_url.clone());
            match resolved {
                UpdateAction::Check { force } => {
                    let path = if force { "/api/update/check?force=true" } else { "/api/update/check" };
                    ("GET".into(), path.into(), None)
                }
                UpdateAction::Install { auto, manifest_url } => {
                    let mut qs = String::new();
                    if auto {
                        qs.push_str("?method=auto");
                    }
                    if let Some(m) = manifest_url {
                        if qs.is_empty() {
                            qs.push('?');
                        } else {
                            qs.push('&');
                        }
                        qs.push_str(&format!("manifest_url={}", urlencoding::encode(&m)));
                    }
                    ("POST".into(), format!("/api/update/install{}", qs), None)
                }
                UpdateAction::Channel { channel } => match channel {
                    None => ("GET".into(), "/api/update/channel".into(), None),
                    Some(name) => ("POST".into(), "/api/update/channel".into(),
                        Some(serde_json::json!({"channel": name}).to_string())),
                },
            }
        }
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
            MeshAction::Broadcast => ("POST".into(), "/api/mesh/broadcast/start".into(), None),
            MeshAction::StopBroadcast => ("POST".into(), "/api/mesh/broadcast/stop".into(), None),
            MeshAction::Refresh => ("POST".into(), "/api/mesh/refresh".into(), None),
            MeshAction::Announce { to } => {
                let path = match to {
                    Some(t) => format!("/api/mesh/announce/all?to={}", urlencoding::encode(t)),
                    None => "/api/mesh/announce/all".into(),
                };
                ("POST".into(), path, None)
            }
            // peer set-addr is local-only — relay path is meaningless. Map
            // to the local endpoint so it errors clearly if relayed.
            MeshAction::Peer { action } => match action {
                MeshPeerAction::SetAddr { peer, addr } => {
                    let (ip, port) = parse_ip_port(addr);
                    let mut body = serde_json::json!({"ip": ip});
                    if let Some(p) = port { body["port"] = serde_json::json!(p); }
                    ("POST".into(), format!("/api/mesh/peers/{}/addr", urlencoding::encode(peer)),
                        Some(body.to_string()))
                }
            },
            // Remote relay commands — these map to generic API calls
            _ => ("GET".into(), "/api/mesh/status".into(), None),
        },
        // Bridge, Theme — best effort
        Commands::Bridge { .. } => ("GET".into(), "/api/health".into(), None),
        Commands::Theme { action } => match action {
            super::ThemeAction::List => ("GET".into(), "/api/themes".into(), None),
            _ => ("GET".into(), "/api/themes".into(), None),
        },
        // Mem — relay to /api/mem/stats as a safe default. Direct relay of
        // mem operations isn't routed through mesh in 3.1; users should run
        // `codeterm mem ...` on the local pane (mem is per-machine state).
        Commands::Mem { .. } => ("GET".into(), "/api/mem/stats".into(), None),
        Commands::Sessions { action } => match action {
            super::SessionsAction::Search { query, provider, project, limit, max_snippets } => {
                let mut path = format!("/api/sessions/search?q={}", urlencoding::encode(query));
                if let Some(p) = provider { path.push_str(&format!("&provider={}", urlencoding::encode(p))); }
                if let Some(p) = project { path.push_str(&format!("&project={}", urlencoding::encode(p))); }
                path.push_str(&format!("&limit={}&max_snippets={}", limit, max_snippets));
                ("GET".into(), path, None)
            }
            super::SessionsAction::Resume { session_id, title, workspace_id, workspace_name } => {
                let mut body = serde_json::Map::new();
                if let Some(t) = title { body.insert("title".into(), serde_json::Value::String(t.clone())); }
                if let Some(w) = workspace_id { body.insert("workspace_id".into(), serde_json::Value::String(w.clone())); }
                if let Some(n) = workspace_name { body.insert("workspace_name".into(), serde_json::Value::String(n.clone())); }
                (
                    "POST".into(),
                    format!("/api/sessions/{}/resume", urlencoding::encode(session_id)),
                    Some(serde_json::Value::Object(body).to_string()),
                )
            }
            super::SessionsAction::Get { session_id } => (
                "GET".into(),
                format!("/api/sessions/{}", urlencoding::encode(session_id)),
                None,
            ),
        },
        Commands::Keepawake { action } => match action {
            Some(super::KeepAwakeAction::Stop) => ("POST".into(), "/api/keepawake/stop".into(), None),
            Some(super::KeepAwakeAction::Start { duration_secs, display }) => (
                "POST".into(),
                "/api/keepawake/start".into(),
                Some(serde_json::json!({
                    "duration_secs": duration_secs,
                    "display": display,
                }).to_string()),
            ),
            _ => ("GET".into(), "/api/keepawake".into(), None),
        },
        Commands::SelfUpdate { check, rollback, channel, .. } => {
            if *check {
                ("GET".into(), "/api/update/check".into(), None)
            } else if *rollback {
                ("POST".into(), "/api/update/rollback".into(), None)
            } else {
                let path = match channel {
                    Some(c) if !c.is_empty() => format!(
                        "/api/update/self_apply?channel={}",
                        urlencoding::encode(c)
                    ),
                    _ => "/api/update/self_apply".to_string(),
                };
                ("POST".into(), path, None)
            }
        }
    }
}

/// Shortcut relay — maps old-style actions to new generic relay format.
pub fn mesh_relay_cmd(http: &reqwest::blocking::Client, base: &str, token: &Option<String>, target: &str, action: &str, payload: &str, pane: &str, json_mode: bool) {
    let (method, path, body) = match action {
        "exec" | "send" => {
            let source = std::env::var("CODETERM_PANE_ID").unwrap_or_else(|_| "unknown".to_string());
            ("POST", format!("/api/panes/{}/send", if pane.is_empty() { "general" } else { pane }),
                serde_json::json!({"text": payload, "enter": true, "source_pane": source}))
        }
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
                } else if let Some(pane_text) = data.get("text").and_then(|v| v.as_str()) {
                    for line in pane_text.lines() {
                        if !line.trim().is_empty() {
                            println!("{}", line);
                        }
                    }
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

/// Split an "ip[:port]" string into (ip, optional port). Best-effort —
/// IPv6 with brackets is supported, plain IPv4 with optional ":port" is
/// the typical case.
pub fn parse_ip_port(input: &str) -> (String, Option<u16>) {
    let s = input.trim();
    // [ipv6]:port form
    if let Some(rest) = s.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let ip = rest[..end].to_string();
            let tail = &rest[end + 1..];
            let port = tail.strip_prefix(':').and_then(|p| p.parse::<u16>().ok());
            return (ip, port);
        }
    }
    // ipv4[:port] — split on the LAST colon to avoid breaking bare ipv6
    if let Some(idx) = s.rfind(':') {
        let (ip, port_str) = s.split_at(idx);
        let port_str = &port_str[1..];
        if !ip.contains(':') {
            // Plain ipv4 with port
            if let Ok(p) = port_str.parse::<u16>() {
                return (ip.to_string(), Some(p));
            }
        }
    }
    (s.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Commands, UpdateAction};

    fn update_cmd(
        action: Option<UpdateAction>,
        check: bool,
        apply: bool,
        channel: Option<String>,
        manifest_url: Option<String>,
    ) -> Commands {
        Commands::Update { action, check, apply, channel, manifest_url }
    }

    #[test]
    fn check_maps_to_get_check() {
        let (m, p, b) = cli_command_to_api(&update_cmd(None, true, false, None, None));
        assert_eq!(m, "GET");
        assert_eq!(p, "/api/update/check");
        assert!(b.is_none());
    }

    #[test]
    fn check_force_maps_to_query_param() {
        let (m, p, _) = cli_command_to_api(&update_cmd(
            Some(UpdateAction::Check { force: true }),
            false, false, None, None,
        ));
        assert_eq!(m, "GET");
        assert_eq!(p, "/api/update/check?force=true");
    }

    #[test]
    fn apply_maps_to_install_auto() {
        let (m, p, b) = cli_command_to_api(&update_cmd(None, false, true, None, None));
        assert_eq!(m, "POST");
        assert_eq!(p, "/api/update/install?method=auto");
        assert!(b.is_none());
    }

    #[test]
    fn apply_with_manifest_url_appends_query() {
        let (m, p, _) = cli_command_to_api(&update_cmd(
            None, false, true, None,
            Some("http://localhost:9999/latest.json".to_string()),
        ));
        assert_eq!(m, "POST");
        assert!(p.starts_with("/api/update/install?method=auto&manifest_url="));
        assert!(p.contains("localhost%3A9999"));
    }

    #[test]
    fn channel_get_maps_to_get_channel() {
        let (m, p, b) = cli_command_to_api(&update_cmd(
            None, false, false, Some(String::new()), None,
        ));
        assert_eq!(m, "GET");
        assert_eq!(p, "/api/update/channel");
        assert!(b.is_none());
    }

    #[test]
    fn channel_set_maps_to_post_channel_with_body() {
        let (m, p, b) = cli_command_to_api(&update_cmd(
            None, false, false, Some("rc".to_string()), None,
        ));
        assert_eq!(m, "POST");
        assert_eq!(p, "/api/update/channel");
        let body = b.expect("body");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["channel"], "rc");
    }

    #[test]
    fn self_update_apply_maps_to_post_self_apply() {
        let (m, p, b) = cli_command_to_api(&Commands::SelfUpdate {
            check: false, apply: true, rollback: false, channel: None,
        });
        assert_eq!(m, "POST");
        assert_eq!(p, "/api/update/self_apply");
        assert!(b.is_none());
    }

    #[test]
    fn self_update_apply_with_channel_appends_query() {
        let (m, p, _) = cli_command_to_api(&Commands::SelfUpdate {
            check: false, apply: true, rollback: false,
            channel: Some("rc".to_string()),
        });
        assert_eq!(m, "POST");
        assert_eq!(p, "/api/update/self_apply?channel=rc");
    }

    #[test]
    fn self_update_check_maps_to_check_endpoint() {
        let (m, p, _) = cli_command_to_api(&Commands::SelfUpdate {
            check: true, apply: false, rollback: false, channel: None,
        });
        assert_eq!(m, "GET");
        assert_eq!(p, "/api/update/check");
    }

    #[test]
    fn self_update_rollback_maps_to_rollback_endpoint() {
        let (m, p, _) = cli_command_to_api(&Commands::SelfUpdate {
            check: false, apply: false, rollback: true, channel: None,
        });
        assert_eq!(m, "POST");
        assert_eq!(p, "/api/update/rollback");
    }

    #[test]
    fn mesh_refresh_maps_to_post_refresh() {
        let (m, p, b) = cli_command_to_api(&Commands::Mesh { action: MeshAction::Refresh });
        assert_eq!(m, "POST");
        assert_eq!(p, "/api/mesh/refresh");
        assert!(b.is_none());
    }

    #[test]
    fn mesh_announce_no_target_maps_to_announce_all() {
        let (m, p, b) = cli_command_to_api(&Commands::Mesh {
            action: MeshAction::Announce { to: None },
        });
        assert_eq!(m, "POST");
        assert_eq!(p, "/api/mesh/announce/all");
        assert!(b.is_none());
    }

    #[test]
    fn mesh_announce_to_target_appends_query_param() {
        let (m, p, _) = cli_command_to_api(&Commands::Mesh {
            action: MeshAction::Announce { to: Some("BLACKSTAR".into()) },
        });
        assert_eq!(m, "POST");
        assert_eq!(p, "/api/mesh/announce/all?to=BLACKSTAR");
    }

    #[test]
    fn parse_ip_port_plain_ipv4() {
        assert_eq!(parse_ip_port("192.168.1.1"), ("192.168.1.1".to_string(), None));
    }

    #[test]
    fn parse_ip_port_ipv4_with_port() {
        assert_eq!(parse_ip_port("100.84.1.2:7685"), ("100.84.1.2".to_string(), Some(7685)));
    }

    #[test]
    fn parse_ip_port_ipv6_bracketed_with_port() {
        let (ip, port) = parse_ip_port("[fe80::1]:7685");
        assert_eq!(ip, "fe80::1");
        assert_eq!(port, Some(7685));
    }

    #[test]
    fn parse_ip_port_bare_ipv6_no_port() {
        let (ip, port) = parse_ip_port("fe80::1");
        assert_eq!(ip, "fe80::1");
        assert_eq!(port, None);
    }

    #[test]
    fn mesh_peer_set_addr_serializes_body_with_port() {
        let (m, p, b) = cli_command_to_api(&Commands::Mesh {
            action: MeshAction::Peer {
                action: MeshPeerAction::SetAddr {
                    peer: "BLACKSTAR".into(),
                    addr: "100.84.159.37:7685".into(),
                },
            },
        });
        assert_eq!(m, "POST");
        assert_eq!(p, "/api/mesh/peers/BLACKSTAR/addr");
        let body = b.expect("body");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["ip"], "100.84.159.37");
        assert_eq!(v["port"], 7685);
    }

    #[test]
    fn mesh_peer_set_addr_omits_port_when_absent() {
        let (_, _, b) = cli_command_to_api(&Commands::Mesh {
            action: MeshAction::Peer {
                action: MeshPeerAction::SetAddr {
                    peer: "X".into(),
                    addr: "10.0.0.1".into(),
                },
            },
        });
        let v: serde_json::Value = serde_json::from_str(&b.unwrap()).unwrap();
        assert_eq!(v["ip"], "10.0.0.1");
        assert!(v.get("port").is_none());
    }
}
