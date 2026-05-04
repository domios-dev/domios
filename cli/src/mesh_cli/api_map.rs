//! Translate any local CLI command to the (method, path, body) tuple needed
//! to forward it through /api/mesh/relay to a paired peer's API.
//!
//! One large match — the dispatch table for every CLI subcommand. Adding a
//! new subcommand to `super::Commands` requires adding an arm here.

use crate::{Commands, UpdateAction};

use super::{parse_ip_port, MeshAction, MeshPeerAction};
use super::update::resolve_update_action;

/// Map a CLI command to its corresponding API call.
pub fn cli_command_to_api(cmd: &Commands) -> (String, String, Option<String>) {
    match cmd {
        Commands::Identify => ("GET".into(), "/api/identify".into(), None),
        Commands::Capabilities => ("GET".into(), "/api/identify".into(), None),
        Commands::GetOutput { pane, lines } => {
            let p = pane.as_deref().unwrap_or("general");
            ("GET".into(), format!("/api/panes/{}/output?lines={}&strip_ansi=true", p, lines), None)
        }
        Commands::SendText { text, pane, from_file, from_stdin } => {
            let p = pane.as_deref().unwrap_or("general");
            let source = std::env::var("CODETERM_PANE_ID").unwrap_or_else(|_| "unknown".to_string());
            let body_text = if *from_stdin {
                use std::io::Read;
                let mut buf = String::new();
                if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                    eprintln!("Error: failed to read --from-stdin: {}", e);
                    std::process::exit(1);
                }
                buf
            } else if let Some(path) = from_file.as_ref() {
                std::fs::read_to_string(path).unwrap_or_else(|_| text.clone())
            } else {
                text.clone()
            };
            if body_text.trim().is_empty() {
                eprintln!("Error: send-text payload is empty (mesh path)");
                eprintln!("hint: --from-stdin via heredoc may have dropped under your shell wrapper;");
                eprintln!("      try --from-file <path> with a real tempfile.");
                std::process::exit(2);
            }
            ("POST".into(), format!("/api/panes/{}/send", p),
                Some(serde_json::json!({"text": body_text, "enter": true, "source_pane": source}).to_string()))
        }
        Commands::Notify { title, body, level } => {
            ("POST".into(), "/api/notifications".into(),
                Some(serde_json::json!({"title": title, "body": body, "level": level}).to_string()))
        }
        Commands::Workspace { action } => match action {
            crate::WorkspaceAction::List => ("GET".into(), "/api/workspaces".into(), None),
            crate::WorkspaceAction::Create { name, .. } => ("POST".into(), "/api/workspaces".into(),
                Some(serde_json::json!({"name": name}).to_string())),
            crate::WorkspaceAction::Focus { id } => ("POST".into(), format!("/api/workspaces/{}/focus", id), None),
            crate::WorkspaceAction::Rename { id, name } => ("POST".into(), format!("/api/workspaces/{}/rename", id),
                Some(serde_json::json!({"name": name}).to_string())),
            crate::WorkspaceAction::Close { id } => ("POST".into(), format!("/api/workspaces/{}/close", id), None),
            _ => ("GET".into(), "/api/workspaces".into(), None),
        },
        Commands::Pane { action } => match action {
            crate::PaneAction::List => ("GET".into(), "/api/panes".into(), None),
            crate::PaneAction::Current { .. } => ("GET".into(), "/api/panes".into(), None),
            crate::PaneAction::Focus { id } => ("POST".into(), format!("/api/panes/{}/focus", id), None),
            crate::PaneAction::Close { id } => ("POST".into(), format!("/api/panes/{}/close", id), None),
            crate::PaneAction::Rename { title, pane } => {
                let p = pane.as_deref().unwrap_or("general");
                ("POST".into(), format!("/api/panes/{}/rename", p),
                    Some(serde_json::json!({"title": title}).to_string()))
            }
            crate::PaneAction::GetText { pane, formatted } => {
                let p = pane.as_deref().unwrap_or("general");
                ("GET".into(), format!("/api/panes/{}/text?formatted={}", p, formatted), None)
            }
            crate::PaneAction::Exec { command, pane, timeout } => {
                let p = pane.as_deref().unwrap_or("general");
                ("POST".into(), format!("/api/panes/{}/exec", p),
                    Some(serde_json::json!({
                        "command": command,
                        "timeout_ms": timeout,
                        "strip_ansi": true,
                    }).to_string()))
            }
            crate::PaneAction::Status { pane } => {
                let p = pane.as_deref().unwrap_or("general");
                ("GET".into(), format!("/api/panes/{}/status", p), None)
            }
        },
        Commands::Agent { action } => match action {
            crate::AgentAction::List => ("GET".into(), "/api/agents".into(), None),
            crate::AgentAction::Hierarchy => ("GET".into(), "/api/agents/hierarchy".into(), None),
            crate::AgentAction::Spawn { agent, title, task, role, parent, workspace, can_spawn, cwd, args, model, .. } => {
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
                if let Some(m) = model { body["model"] = serde_json::json!(m); }
                ("POST".into(), "/api/agents/spawn".into(), Some(body.to_string()))
            }
            _ => ("GET".into(), "/api/agents".into(), None),
        },
        Commands::Notifications { unread, .. } => {
            let qs = if *unread { "?unread=true" } else { "" };
            ("GET".into(), format!("/api/notifications{}", qs), None)
        }
        Commands::Browser { action } => match action {
            crate::BrowserAction::Open { url } => ("POST".into(), "/api/browser/open".into(),
                Some(serde_json::json!({"url": url}).to_string())),
        },
        Commands::Cron { action } => match action {
            crate::CronAction::List { .. } => ("GET".into(), "/api/crons".into(), None),
            crate::CronAction::Create { name, schedule, task, workspace, agent, model, cwd, description, .. } => {
                let mut body = serde_json::json!({"name": name, "schedule": schedule, "task": task, "agent": agent});
                if let Some(m) = model { body["model"] = serde_json::json!(m); }
                if let Some(w) = workspace { body["workspace_id"] = serde_json::json!(w); }
                if let Some(c) = cwd { body["cwd"] = serde_json::json!(c); }
                if let Some(d) = description { body["description"] = serde_json::json!(d); }
                ("POST".into(), "/api/crons".into(), Some(body.to_string()))
            }
            crate::CronAction::Delete { id } => ("DELETE".into(), format!("/api/crons/{}", id), None),
            crate::CronAction::Enable { id } => ("POST".into(), format!("/api/crons/{}/enable", id), None),
            crate::CronAction::Disable { id } => ("POST".into(), format!("/api/crons/{}/disable", id), None),
            crate::CronAction::Trigger { id } => ("POST".into(), format!("/api/crons/{}/trigger", id), None),
            crate::CronAction::Cancel { id } => ("POST".into(), format!("/api/crons/{}/cancel", id), None),
            crate::CronAction::History { id } => ("GET".into(), format!("/api/crons/{}/history", id), None),
            _ => ("GET".into(), "/api/crons".into(), None),
        },
        Commands::Config { action } => match action {
            crate::ConfigAction::Get { key } => ("GET".into(), format!("/api/config?key={}", key), None),
            crate::ConfigAction::Set { key, value } => {
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
                UpdateAction::Install { auto, manifest_url, from_file: _, continue_prompt: _, continue_pane: _ } => {
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
            _ => ("GET".into(), "/api/mesh/status".into(), None),
        },
        Commands::Bridge { .. } => ("GET".into(), "/api/health".into(), None),
        Commands::Theme { action } => match action {
            crate::ThemeAction::List => ("GET".into(), "/api/themes".into(), None),
            _ => ("GET".into(), "/api/themes".into(), None),
        },
        // Mem operations are per-machine state; relay to /api/mem/stats as a
        // safe default. Users should run `codeterm mem ...` on the local pane.
        Commands::Mem { .. } => ("GET".into(), "/api/mem/stats".into(), None),
        Commands::Sessions { action } => match action {
            crate::SessionsAction::Search { query, provider, project, limit, max_snippets } => {
                let mut path = format!("/api/sessions/search?q={}", urlencoding::encode(query));
                if let Some(p) = provider { path.push_str(&format!("&provider={}", urlencoding::encode(p))); }
                if let Some(p) = project { path.push_str(&format!("&project={}", urlencoding::encode(p))); }
                path.push_str(&format!("&limit={}&max_snippets={}", limit, max_snippets));
                ("GET".into(), path, None)
            }
            crate::SessionsAction::Resume { session_id, title, workspace_id, workspace_name } => {
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
            crate::SessionsAction::Get { session_id } => (
                "GET".into(),
                format!("/api/sessions/{}", urlencoding::encode(session_id)),
                None,
            ),
        },
        Commands::Keepawake { action } => match action {
            Some(crate::KeepAwakeAction::Stop) => ("POST".into(), "/api/keepawake/stop".into(), None),
            Some(crate::KeepAwakeAction::Start { duration_secs, display }) => (
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
