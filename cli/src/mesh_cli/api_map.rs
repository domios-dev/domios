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
        Commands::SendText { text, pane, from_file, from_stdin, no_wait } => {
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
            let mut body = serde_json::json!({"text": body_text, "enter": true, "source_pane": source});
            if *no_wait {
                body["verify"] = serde_json::json!(false);
            }
            ("POST".into(), format!("/api/panes/{}/send", p), Some(body.to_string()))
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
            crate::PaneAction::New { workspace, cwd, title, cmd: _ } => {
                let mut body = serde_json::json!({});
                if let Some(w) = workspace { body["workspace_id"] = serde_json::Value::String(w.clone()); }
                if let Some(c) = cwd { body["cwd"] = serde_json::Value::String(c.clone()); }
                if let Some(t) = title { body["title"] = serde_json::Value::String(t.clone()); }
                ("POST".into(), "/api/panes".into(), Some(body.to_string()))
            }
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
            crate::AgentAction::Spawn { agent, title, task, role, parent, workspace, can_spawn, cwd, args, model, no_wait } => {
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
                if *no_wait { body["wait_ready_secs"] = serde_json::json!(0); }
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
        Commands::Bridge { action } => match action {
            crate::BridgeAction::Restart => (
                "POST".into(),
                "/api/bridge/start".into(),
                Some(serde_json::json!({
                    "service": "telegram",
                    "ensure_token": true,
                    "wait_for_lock_ms": 10000,
                }).to_string()),
            ),
            crate::BridgeAction::Send { text, to, append, .. } => (
                "POST".into(),
                "/api/bridge/send".into(),
                Some(serde_json::json!({
                    "text": text,
                    "chat_id": to,
                    "append": append,
                }).to_string()),
            ),
            // React over mesh isn't relayable today (no /api/bridge/react endpoint).
            crate::BridgeAction::React { .. } => ("GET".into(), "/api/health".into(), None),
            // Migrate is intercepted in main.rs before reaching the relay path.
            crate::BridgeAction::Migrate { .. } => ("GET".into(), "/api/health".into(), None),
        },
        Commands::Theme { action } => match action {
            crate::ThemeAction::List => ("GET".into(), "/api/themes".into(), None),
            _ => ("GET".into(), "/api/themes".into(), None),
        },
        Commands::Mem { action } => mem_to_api(action),
        Commands::Sessions { action } => match action {
            crate::SessionsAction::Search { query, provider, project, limit, max_snippets } => {
                let mut path = format!("/api/sessions/search?q={}", urlencoding::encode(query));
                if let Some(p) = provider { path.push_str(&format!("&provider={}", urlencoding::encode(p))); }
                if let Some(p) = project { path.push_str(&format!("&project={}", urlencoding::encode(p))); }
                path.push_str(&format!("&limit={}&max_snippets={}", limit, max_snippets));
                ("GET".into(), path, None)
            }
            crate::SessionsAction::Resume { session_id, title, workspace_id, workspace_name, model } => {
                let mut body = serde_json::Map::new();
                if let Some(t) = title { body.insert("title".into(), serde_json::Value::String(t.clone())); }
                if let Some(w) = workspace_id { body.insert("workspace_id".into(), serde_json::Value::String(w.clone())); }
                if let Some(n) = workspace_name { body.insert("workspace_name".into(), serde_json::Value::String(n.clone())); }
                if let Some(m) = model { body.insert("model".into(), serde_json::Value::String(m.clone())); }
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
        // `system` writes platform-specific files (schtasks/launchd/systemd)
        // and only makes sense on the local host. Mesh relay falls through
        // to /identify so callers always get a valid JSON response.
        Commands::System { .. } => ("GET".into(), "/api/identify".into(), None),
    }
}

fn mem_to_api(action: &crate::MemAction) -> (String, String, Option<String>) {
    use crate::{MemAction, MemGraphAction};
    use serde_json::json;

    let enc = urlencoding::encode;
    match action {
        MemAction::Search { query, kind, area, scope, limit, .. } => {
            let mut path = format!("/api/mem/search?q={}&limit={}", enc(query), limit);
            if let Some(k) = kind { path.push_str(&format!("&kind={}", enc(k))); }
            if let Some(a) = area { path.push_str(&format!("&area={}", enc(a))); }
            if let Some(s) = scope { path.push_str(&format!("&scope={}", enc(s))); }
            ("GET".into(), path, None)
        }
        MemAction::Get { id } => ("GET".into(), format!("/api/mem/items/{}", enc(id)), None),
        MemAction::Delete { id } => ("DELETE".into(), format!("/api/mem/items/{}", enc(id)), None),
        MemAction::History { id } => ("GET".into(), format!("/api/mem/items/{}/history", enc(id)), None),
        MemAction::Taxonomy { workspace } => {
            let path = match workspace {
                Some(w) => format!("/api/mem/taxonomy?workspace={}", enc(w)),
                None => "/api/mem/taxonomy".into(),
            };
            ("GET".into(), path, None)
        }
        MemAction::Tail { limit, .. } => {
            ("GET".into(), format!("/api/mem/tail?limit={}", limit), None)
        }
        MemAction::Update { id, title, summary, area, tags, body, importance } => {
            let mut payload = serde_json::Map::new();
            if let Some(v) = title { payload.insert("title".into(), json!(v)); }
            if let Some(v) = summary { payload.insert("summary".into(), json!(v)); }
            if let Some(v) = area { payload.insert("area".into(), json!(v)); }
            if let Some(v) = tags { payload.insert("tags".into(), json!(v)); }
            if let Some(v) = body { payload.insert("body".into(), json!(v)); }
            if let Some(v) = importance { payload.insert("importance".into(), json!(v)); }
            ("PATCH".into(), format!("/api/mem/items/{}", enc(id)),
                Some(serde_json::Value::Object(payload).to_string()))
        }
        MemAction::Save { kind, title, body, summary, tags, area, importance, scope, source_path, replace, skip_extract } => {
            let mut payload = json!({"kind": kind, "importance": importance});
            if let Some(v) = title { payload["title"] = json!(v); }
            if let Some(v) = body { payload["body"] = json!(v); }
            if let Some(v) = summary { payload["summary"] = json!(v); }
            if let Some(v) = tags { payload["tags"] = json!(v); }
            if let Some(v) = area { payload["area"] = json!(v); }
            if let Some(v) = scope { payload["scope"] = json!(v); }
            if let Some(v) = source_path { payload["source_path"] = json!(v); }
            if *replace { payload["replace"] = json!(true); }
            if *skip_extract { payload["skip_extract"] = json!(true); }
            ("POST".into(), "/api/mem/items".into(), Some(payload.to_string()))
        }
        MemAction::WakeUp { budget, l0_only, no_l0 } => {
            let mut path = String::from("/api/mem/wake-up");
            let mut sep = '?';
            if let Some(b) = budget { path.push(sep); sep = '&'; path.push_str(&format!("budget={}", b)); }
            if *l0_only { path.push(sep); sep = '&'; path.push_str("l0_only=true"); }
            if *no_l0 { path.push(sep); let _ = sep; path.push_str("no_l0=true"); }
            ("GET".into(), path, None)
        }
        MemAction::ExtractPending { limit } => {
            ("POST".into(), format!("/api/mem/extract-pending?limit={}", limit), None)
        }
        MemAction::Stats => ("GET".into(), "/api/mem/stats".into(), None),
        MemAction::Learn { text, kind, tags, error, feature } => {
            let resolved_text = text.clone().or_else(|| error.clone()).or_else(|| feature.clone()).unwrap_or_default();
            let resolved_kind = kind.clone()
                .or_else(|| if error.is_some() { Some("error".into()) } else { None })
                .or_else(|| if feature.is_some() { Some("feature".into()) } else { None })
                .unwrap_or_else(|| "insight".into());
            let mut payload = json!({"text": resolved_text, "kind": resolved_kind});
            if let Some(v) = tags { payload["tags"] = json!(v); }
            ("POST".into(), "/api/mem/learn".into(), Some(payload.to_string()))
        }
        MemAction::LearnList => {
            ("GET".into(), "/api/mem/search?kind=learning&limit=50".into(), None)
        }
        MemAction::LearnPromote { id } => {
            ("POST".into(), format!("/api/mem/promote/{}", enc(id)), None)
        }
        MemAction::Sync { path } => {
            let url = match path {
                Some(p) => format!("/api/mem/sync?path={}", enc(p)),
                None => "/api/mem/sync".into(),
            };
            ("POST".into(), url, None)
        }
        MemAction::Dream { action } => {
            use crate::MemDreamAction;
            match action {
                MemDreamAction::Run { phase } => (
                    "POST".into(),
                    format!("/api/mem/dream?phase={}", enc(phase)),
                    None,
                ),
                // `apply` and `diary` open the DB directly. They never round-
                // trip via mesh — fall through to /stats so callers still get
                // a valid JSON response.
                MemDreamAction::Apply { .. } | MemDreamAction::Diary { .. } => {
                    ("GET".into(), "/api/mem/stats".into(), None)
                }
            }
        }
        MemAction::Graph { action } => match action {
            MemGraphAction::Create { id, entity_type, props, scope } => {
                let props_val: serde_json::Value = serde_json::from_str(props).unwrap_or(json!({}));
                let mut payload = json!({"id": id, "entity_type": entity_type, "props": props_val});
                if let Some(s) = scope { payload["scope"] = json!(s); }
                ("POST".into(), "/api/mem/graph/entities".into(), Some(payload.to_string()))
            }
            MemGraphAction::Relate { from, rel, to, valid_from, valid_to, confidence, scope } => {
                let mut payload = json!({"from": from, "rel": rel, "to": to, "confidence": confidence});
                if let Some(v) = valid_from { payload["valid_from"] = json!(v); }
                if let Some(v) = valid_to { payload["valid_to"] = json!(v); }
                if let Some(v) = scope { payload["scope"] = json!(v); }
                ("POST".into(), "/api/mem/graph/relate".into(), Some(payload.to_string()))
            }
            MemGraphAction::Related { id, rel, as_of, include_invalidated } => {
                let mut path = format!("/api/mem/graph/related?id={}", enc(id));
                if let Some(r) = rel { path.push_str(&format!("&rel={}", enc(r))); }
                if let Some(a) = as_of { path.push_str(&format!("&as_of={}", enc(a))); }
                if *include_invalidated { path.push_str("&include_invalidated=true"); }
                ("GET".into(), path, None)
            }
            MemGraphAction::Invalidate { from, rel, to, ended } => {
                let mut payload = json!({"from": from, "rel": rel, "to": to});
                if let Some(e) = ended { payload["ended"] = json!(e); }
                ("POST".into(), "/api/mem/graph/invalidate".into(), Some(payload.to_string()))
            }
            MemGraphAction::Timeline { entity, include_invalidated } => {
                let mut path = format!("/api/mem/graph/timeline?entity={}", enc(entity));
                if *include_invalidated { path.push_str("&include_invalidated=true"); }
                ("GET".into(), path, None)
            }
            MemGraphAction::Traverse { from, hops, max_nodes, as_of, include_invalidated } => {
                let mut path = format!("/api/mem/graph/traverse?from={}&hops={}&max_nodes={}", enc(from), hops, max_nodes);
                if let Some(a) = as_of { path.push_str(&format!("&as_of={}", enc(a))); }
                if *include_invalidated { path.push_str("&include_invalidated=true"); }
                ("GET".into(), path, None)
            }
            MemGraphAction::Path { from, to, max_hops, include_invalidated } => {
                let mut path = format!("/api/mem/graph/path?from={}&to={}&max_hops={}", enc(from), enc(to), max_hops);
                if *include_invalidated { path.push_str("&include_invalidated=true"); }
                ("GET".into(), path, None)
            }
            MemGraphAction::Tunnels { include_invalidated } => {
                let path = if *include_invalidated {
                    "/api/mem/graph/tunnels?include_invalidated=true".to_string()
                } else {
                    "/api/mem/graph/tunnels".to_string()
                };
                ("GET".into(), path, None)
            }
            MemGraphAction::Alias { .. } => ("POST".into(), "/api/mem/graph/alias".into(), None),
            // Local-only graph mutations: no mesh relay. Fall through to
            // /stats so the call still returns valid JSON if a caller
            // somehow routes it across the wire.
            MemGraphAction::Route { .. }
            | MemGraphAction::PurgeInvalidated { .. }
            | MemGraphAction::ReviveBroken { .. }
            | MemGraphAction::IntroduceTopic { .. }
            | MemGraphAction::Hubs { .. }
            | MemGraphAction::Get { .. } => {
                ("GET".into(), "/api/mem/stats".into(), None)
            }
        },
        // Local-only subcommands. Mesh relay falls through to /stats so callers
        // always get a valid JSON response, but the operation itself never
        // makes sense across the wire (PTY scrollback / subprocess /
        // filesystem skill folders / env-coupled worker handles). `Secret` is
        // already hard-rejected upstream in main.rs.
        MemAction::Flush { .. }
        | MemAction::RunWorker { .. }
        | MemAction::Skill { .. }
        | MemAction::Worker { .. }
        | MemAction::Secret { .. }
        | MemAction::Repair { .. } => ("GET".into(), "/api/mem/stats".into(), None),
        MemAction::Reembed { scope } => (
            "POST".into(),
            format!("/api/mem/reembed?scope={}", enc(scope)),
            None,
        ),
    }
}
