use crate::transport::ApiVerdict;
use crate::{
    cmd::{delayed::parse_schedule, send_text::resolve_send_text_input},
    Commands, TeamAction, UpdateAction,
};

use super::update::resolve_update_action;
use super::{parse_ip_port, MeshAction, MeshPeerAction};

mod actions;

pub(crate) use actions::send_text_to_api;
use actions::{
    agent_to_api, delayed_create_to_api, mem_to_api, now_ms, service_to_api, team_pair_to_api,
};

pub const UNSUPPORTED_OVER_MESH: &str = "UNSUPPORTED";

fn unsupported(reason: &str) -> (String, String, Option<String>) {
    (UNSUPPORTED_OVER_MESH.into(), reason.into(), None)
}

/// Support is declared once in `crate::transport`; this module only builds the
/// REST request for the commands the table says have one.
pub fn cli_command_to_api(cmd: &Commands) -> (String, String, Option<String>) {
    match crate::transport::transport_for(cmd).api_verdict {
        ApiVerdict::Unsupported(reason) => unsupported(reason),
        // `debug` refuses with its own message and exit code rather than a
        // verdict; the table declares that shape instead of normalising it.
        ApiVerdict::RefusedInPlace => {
            eprintln!("Error: `debug` is local-only — refusing to crash a remote peer via --mesh");
            std::process::exit(2);
        }
        ApiVerdict::Rest => rest_request(cmd),
    }
}

fn rest_request(cmd: &Commands) -> (String, String, Option<String>) {
    match cmd {
        Commands::Team {
            action:
                TeamAction::Pair {
                    orchestrator,
                    planner,
                    workspace,
                    title,
                    acknowledge_leaf,
                },
        } => team_pair_to_api(orchestrator, planner, workspace, title, *acknowledge_leaf),
        Commands::Identify => ("GET".into(), "/api/identify".into(), None),
        Commands::SendText {
            text,
            tab,
            from_file,
            from_pad,
            stdin: use_stdin,
            actions,
            delay,
            at,
            require_agent: _,
            to_shell: _,
        } => {
            let mut stdin = std::io::stdin();
            if delay.is_some() || at.is_some() {
                delayed_create_to_api(text, tab, from_file, from_pad, *use_stdin, delay, at, &mut stdin)
            } else {
                send_text_to_api(text, tab, from_file, from_pad, *use_stdin, actions, &mut stdin)
            }
        }
        Commands::Delayed { action } => match action {
            crate::DelayedAction::List { tab } => {
                let p = tab.as_deref().unwrap_or("general");
                (
                    "GET".into(),
                    format!("/api/tabs/{}/delayed-messages", p),
                    None,
                )
            }
            crate::DelayedAction::Create {
                text,
                tab,
                delay,
                at,
                from_file,
                from_pad,
                stdin: use_stdin,
            } => {
                let mut stdin = std::io::stdin();
                delayed_create_to_api(text, tab, from_file, from_pad, *use_stdin, delay, at, &mut stdin)
            }
            crate::DelayedAction::Edit {
                id,
                text,
                delay,
                at,
                from_file,
                from_pad,
                stdin: use_stdin,
            } => {
                let mut body = serde_json::Map::new();
                if text.is_some() || from_file.is_some() || from_pad.is_some() || *use_stdin {
                    match resolve_send_text_input(
                        text.clone(),
                        from_file.clone(),
                        from_pad.clone(),
                        *use_stdin,
                        &mut std::io::stdin(),
                    ) {
                        Ok((text, _)) => {
                            body.insert("text".into(), serde_json::json!(text));
                        }
                        Err(e) => return unsupported(&format!("delayed edit input error: {}", e)),
                    }
                }
                if delay.is_some() || at.is_some() {
                    match parse_schedule(delay.clone(), at.clone(), now_ms()) {
                        Ok(ms) => {
                            body.insert("scheduledAtMs".into(), serde_json::json!(ms));
                        }
                        Err(e) => {
                            return unsupported(&format!("delayed edit schedule error: {}", e))
                        }
                    }
                }
                if body.is_empty() {
                    return unsupported(
                        "delayed edit needs --text/--from-file/--stdin and/or --delay/--at",
                    );
                }
                (
                    "PATCH".into(),
                    format!("/api/delayed-messages/{}", id),
                    Some(serde_json::Value::Object(body).to_string()),
                )
            }
            crate::DelayedAction::Delete { id } => (
                "DELETE".into(),
                format!("/api/delayed-messages/{}", id),
                None,
            ),
            crate::DelayedAction::SendNow { id } => (
                "POST".into(),
                format!("/api/delayed-messages/{}/send-now", id),
                None,
            ),
        },
        Commands::Notify { action } => match action {
            crate::NotifyAction::Send {
                title,
                from_pad,
                severity,
                level,
                event_key,
                choice,
                offer,
                offer_accept,
                ..
            } => {
                let mut body = serde_json::json!({
                    "title": title,
                    "body": from_pad.as_ref().map(|path| {
                        crate::cmd::payload_source::read_from_pad(path).unwrap_or_else(|e| {
                            eprintln!("Error: {e}");
                            std::process::exit(1)
                        })
                    }),
                    "severity": severity,
                    "level": level.map(|l| l.as_str()),
                    "event_key": event_key,
                });
                // Inserted only when offered, because the local path omits the key
                // rather than sending null — the two bodies have to stay identical.
                if let Some(actions) = crate::cmd::notify::choice_actions(choice).unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    std::process::exit(1)
                }) {
                    body["actions"] = serde_json::to_value(actions).unwrap_or_default();
                }
                // Same reason: the local path omits these keys rather than
                // sending null, so the two bodies stay identical.
                if let Some(surface) = offer {
                    body["offer"] = serde_json::Value::String(surface.clone());
                }
                if let Some(accept) = offer_accept {
                    body["offer_accept"] = serde_json::Value::String(accept.clone());
                }
                (
                    "POST".into(),
                    "/api/notifications".into(),
                    Some(body.to_string()),
                )
            }
            crate::NotifyAction::List { unread } => {
                let qs = if *unread { "?unread=true" } else { "" };
                ("GET".into(), format!("/api/notifications{}", qs), None)
            }
            crate::NotifyAction::Clear => {
                ("POST".into(), "/api/notifications/clear".into(), None)
            }
        },
        Commands::Workspace { action } => match action {
            crate::WorkspaceAction::List => ("GET".into(), "/api/workspaces".into(), None),
            crate::WorkspaceAction::Current { .. } => unsupported(
                "workspace current is caller-local — use workspace list on the peer",
            ),
            crate::WorkspaceAction::Create { name, cwd } => {
                let mut body = serde_json::json!({"name": name});
                if let Some(cwd) = cwd {
                    body["cwd"] = serde_json::json!(cwd);
                }
                ("POST".into(), "/api/workspaces".into(), Some(body.to_string()))
            }
            crate::WorkspaceAction::Focus { id } => {
                ("POST".into(), format!("/api/workspaces/{}/focus", id), None)
            }
            crate::WorkspaceAction::Rename { id, name } => (
                "POST".into(),
                format!("/api/workspaces/{}/rename", id),
                Some(serde_json::json!({"name": name}).to_string()),
            ),
            crate::WorkspaceAction::Close { id, force } => {
                ("POST".into(), format!("/api/workspaces/{}/close", id), Some(serde_json::json!({"force": force}).to_string()))
            }
            crate::WorkspaceAction::MoveTab { tab_id, from, to } => (
                "POST".into(),
                format!("/api/workspaces/{from}/tabs/{tab_id}/move"),
                Some(serde_json::json!({"to_workspace_id": to}).to_string()),
            ),
        },
        Commands::Tab { action } => match action {
            crate::TabAction::List {
                workspace,
                all_workspaces,
                agents,
            } => {
                let mut query = vec![format!("agents={agents}")];
                if let Some(workspace) = workspace {
                    query.push(format!("workspace_id={}", urlencoding::encode(workspace)));
                } else if *all_workspaces {
                    query.push("all_workspaces=true".into());
                } else {
                    query.push("focused_workspace=true".into());
                }
                ("GET".into(), format!("/api/tabs?{}", query.join("&")), None)
            }
            crate::TabAction::Inspect { tabs, tab, lifecycle } => (
                "GET".into(),
                format!(
                    "/api/tabs/inspect?tab_ids={}{}",
                    tabs.iter()
                        .chain(tab.iter())
                        .map(|tab| urlencoding::encode(tab).into_owned())
                        .collect::<Vec<_>>()
                        .join(","),
                    if *lifecycle { "&lifecycle=true" } else { "" }
                ),
                None,
            ),
            crate::TabAction::Current { .. } => ("GET".into(), "/api/tabs".into(), None),
            crate::TabAction::New {
                workspace,
                cwd,
                title,
                team,
                no_team,
                cmd,
                eager,
                chat_backend,
                config,
            } => {
                let mut body = serde_json::json!({});
                if let Some(w) = workspace {
                    body["workspace_id"] = serde_json::Value::String(w.clone());
                }
                if let Some(c) = cwd {
                    body["cwd"] = serde_json::Value::String(c.clone());
                }
                if let Some(t) = title {
                    body["title"] = serde_json::Value::String(t.clone());
                }
                if let Some(team_id) = team {
                    body["team_id"] = serde_json::Value::String(team_id.clone());
                    if let Ok(creator_tab_id) = std::env::var("CODETERM_TAB_ID") {
                        body["creator_tab_id"] = serde_json::Value::String(creator_tab_id);
                    }
                }
                if *no_team {
                    body["detach_from_team"] = serde_json::Value::Bool(true);
                }
                if let Some(c) = cmd {
                    body["cmd"] = serde_json::Value::String(c.clone());
                }
                if *eager {
                    body["eager"] = serde_json::Value::Bool(true);
                }
                if let Some(plugin_id) = chat_backend {
                    body["chat_backend"] = serde_json::Value::String(plugin_id.clone());
                    if let Some(cfg) = config {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(cfg) {
                            body["config"] = v;
                        }
                    }
                }
                ("POST".into(), "/api/tabs".into(), Some(body.to_string()))
            }
            crate::TabAction::Focus { id, tab } => {
                let Ok(id) = crate::args::misc::required_tab_selector(id, tab, "focus") else {
                    return unsupported("tab focus requires --tab <ID>");
                };
                ("POST".into(), format!("/api/tabs/{}/focus", id), None)
            }
            crate::TabAction::Close { id, tab } => {
                let Ok(id) = crate::args::misc::required_tab_selector(id, tab, "close") else {
                    return unsupported("tab close requires --tab <ID>");
                };
                ("POST".into(), format!("/api/tabs/{}/close", id), None)
            }
            crate::TabAction::Restart { tab, cwd, cmd } => {
                if cmd.is_some() {
                    return unsupported("remote tab restart does not support --cmd; restart then use codeterm tab exec");
                }
                let id = tab.as_deref().unwrap_or("general");
                (
                    "POST".into(),
                    format!("/api/tabs/{}/restart", id),
                    Some(serde_json::json!({"cwd": cwd}).to_string()),
                )
            }
            crate::TabAction::Rename { title, tab } => {
                let p = tab.as_deref().unwrap_or("general");
                (
                    "POST".into(),
                    format!("/api/tabs/{}/rename", p),
                    Some(serde_json::json!({"title": title}).to_string()),
                )
            }
            crate::TabAction::Describe { summary, tab } => {
                let p = tab.as_deref().unwrap_or("general");
                let description = match crate::cmd::agent::normalize_tab_description(summary) {
                    Ok(Some(d)) => d,
                    Ok(None) => String::new(),
                    Err(e) => return unsupported(&e),
                };
                (
                    "POST".into(),
                    format!("/api/tabs/{}/describe", p),
                    Some(serde_json::json!({"description": description}).to_string()),
                )
            }
            crate::TabAction::GetText { tab, formatted } => {
                let p = tab.as_deref().unwrap_or("general");
                (
                    "GET".into(),
                    format!("/api/tabs/{}/text?formatted={}", p, formatted),
                    None,
                )
            }
            crate::TabAction::Exec {
                command,
                tab,
                timeout,
            } => {
                let p = tab.as_deref().unwrap_or("general");
                (
                    "POST".into(),
                    format!("/api/tabs/{}/exec", p),
                    Some(
                        serde_json::json!({
                            "command": command,
                            "timeout_ms": timeout,
                            "strip_ansi": true,
                        })
                        .to_string(),
                    ),
                )
            }
            crate::TabAction::Status { tab } => {
                let p = tab.as_deref().unwrap_or("general");
                ("GET".into(), format!("/api/tabs/{}/status", p), None)
            }
            crate::TabAction::Respond { option, tab } => {
                let p = tab.as_deref().unwrap_or("general");
                (
                    "POST".into(),
                    format!("/api/tabs/{}/prompt/respond", p),
                    Some(serde_json::json!({"value": option.to_string()}).to_string()),
                )
            }
            crate::TabAction::Rebind { session_id, tab } => {
                let p = tab.as_deref().unwrap_or("general");
                (
                    "POST".into(),
                    format!("/api/tabs/{}/session/rebind", p),
                    Some(serde_json::json!({"session_id": session_id}).to_string()),
                )
            }
            crate::TabAction::Search { .. } => {
                // tab search runs local CLI-side; peer only supplies raw list.
                // Local ranking keeps `Tab list` output stable.
                ("GET".into(), "/api/tabs".into(), None)
            }
            crate::TabAction::Stop { tab } => {
                let p = tab.as_deref().unwrap_or("general");
                (
                    "POST".into(),
                    format!("/api/tabs/{}/stop", p),
                    Some("{}".into()),
                )
            }
            crate::TabAction::Chat { tab, since } => {
                let p = tab.as_deref().unwrap_or("general");
                (
                    "GET".into(),
                    format!("/api/tabs/{}/chat?since={}", p, since),
                    None,
                )
            }
            crate::TabAction::Capture { action } => match action {
                crate::args::misc::TabCaptureAction::Start { tab, out } => {
                    let p = tab.as_deref().unwrap_or("general");
                    (
                        "POST".into(),
                        format!("/api/tabs/{}/capture/start", p),
                        Some(serde_json::json!({ "out": out }).to_string()),
                    )
                }
                crate::args::misc::TabCaptureAction::Stop { tab } => {
                    let p = tab.as_deref().unwrap_or("general");
                    ("POST".into(), format!("/api/tabs/{}/capture/stop", p), None)
                }
            },
        },
        Commands::Paste { action } => match action {
            crate::PasteAction::Put {
                body,
                from_file,
                from_pad,
                from_stdin,
                ttl,
                content_type,
                pop,
            } => {
                let resolved = if let Some(path) = from_pad {
                    crate::cmd::payload_source::read_from_pad(path).unwrap_or_else(|e| {
                        eprintln!("Error: {e}"); std::process::exit(1)
                    })
                } else if *from_stdin {
                    use std::io::Read;
                    let mut buf = String::new();
                    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                        eprintln!("Error: failed to read --from-stdin: {}", e);
                        std::process::exit(1);
                    }
                    buf
                } else if let Some(path) = from_file.as_ref() {
                    // Surface paste read errors explicitly.
                    // Local path exits on failure; /stats fallback used to hide real errors.
                    match std::fs::read_to_string(path) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("Error: failed to read --from-file {}: {}", path, e);
                            std::process::exit(1);
                        }
                    }
                } else {
                    body.clone().unwrap_or_default()
                };
                if resolved.trim().is_empty() {
                    eprintln!("Error: paste put (mesh) body is empty");
                    std::process::exit(2);
                }
                let mut payload = serde_json::json!({"body": resolved});
                if let Some(spec) = ttl.as_deref() {
                    if let Some(ms) = crate::cmd::paste::parse_duration_ms(spec) {
                        payload["ttl_ms"] = serde_json::json!(ms);
                    }
                }
                if let Some(ct) = content_type.as_deref() {
                    payload["content_type"] = serde_json::json!(ct);
                }
                if *pop {
                    payload["pop_on_read"] = serde_json::json!(true);
                }
                (
                    "POST".into(),
                    "/api/pasteboard".into(),
                    Some(payload.to_string()),
                )
            }
            crate::PasteAction::Get { id, pop } => {
                let id = crate::cmd::paste::normalize_paste_id(id);
                let q = if *pop { "?pop=true" } else { "" };
                ("GET".into(), format!("/api/pasteboard/{}{}", id, q), None)
            }
            crate::PasteAction::List => ("GET".into(), "/api/pasteboard".into(), None),
            crate::PasteAction::Rm { id } => {
                let id = crate::cmd::paste::normalize_paste_id(id);
                ("DELETE".into(), format!("/api/pasteboard/{}", id), None)
            }
        },
        Commands::Agent { action } => agent_to_api(action),
        Commands::Cron { action } => match action {
            crate::CronAction::List { .. } => ("GET".into(), "/api/crons".into(), None),
            crate::CronAction::Create {
                name,
                schedule,
                task,
                workspace,
                team,
                no_team,
                agent,
                model,
                cwd,
                skill,
                description,
                report_tab,
                once,
                jitter_max_ms,
                tz,
                timeout_secs,
            } => {
                let body = crate::args::cron::cron_create_body(
                    name,
                    schedule,
                    task,
                    workspace.as_deref().unwrap_or(""),
                    team.as_deref(),
                    *no_team,
                    agent,
                    model.as_deref(),
                    cwd.as_deref(),
                    description.as_deref(),
                    None,
                    report_tab.as_deref(),
                    *once,
                    *jitter_max_ms,
                    tz.as_deref(),
                    *timeout_secs,
                    skill.as_deref(),
                );
                ("POST".into(), "/api/crons".into(), Some(body.to_string()))
            }
            crate::CronAction::Delete { id } => {
                ("DELETE".into(), format!("/api/crons/{}", id), None)
            }
            crate::CronAction::Enable { id } => {
                ("POST".into(), format!("/api/crons/{}/enable", id), None)
            }
            crate::CronAction::Disable { id } => {
                ("POST".into(), format!("/api/crons/{}/disable", id), None)
            }
            crate::CronAction::Trigger { id } => {
                ("POST".into(), format!("/api/crons/{}/trigger", id), None)
            }
            crate::CronAction::Cancel { id } => {
                ("POST".into(), format!("/api/crons/{}/cancel", id), None)
            }
            crate::CronAction::History { id } => {
                ("GET".into(), format!("/api/crons/{}/history", id), None)
            }
            _ => ("GET".into(), "/api/crons".into(), None),
        },
        Commands::Config { action } => match action {
            crate::ConfigAction::Get { key } => (
                "GET".into(),
                crate::transport::pre_socket::config_get_path(key.as_deref()),
                None,
            ),
            crate::ConfigAction::Set { key, value } => {
                let v: serde_json::Value =
                    serde_json::from_str(value).unwrap_or(serde_json::json!(value));
                (
                    "PATCH".into(),
                    "/api/config".into(),
                    Some(serde_json::json!({"key": key, "value": v}).to_string()),
                )
            }
        },
        Commands::Update {
            action,
            manifest_url,
            after_update,
        } => {
            let resolved =
                resolve_update_action(action.as_ref(), manifest_url.clone(), after_update.clone());
            match resolved {
                UpdateAction::Check { force } => {
                    let path = if force {
                        "/api/update/check?force=true"
                    } else {
                        "/api/update/check"
                    };
                    ("GET".into(), path.into(), None)
                }
                UpdateAction::Install {
                    auto,
                    manifest_url,
                    from_file,
                    continue_prompt,
                    continue_tab,
                    no_continue,
                    after_update,
                } => {
                    if from_file.is_some()
                        || continue_prompt.is_some()
                        || continue_tab.is_some()
                        || no_continue
                    {
                        return unsupported("update install --from-file/--continue-prompt/--continue-tab are local-only; run on the target machine");
                    }
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
                    if let Some(t) = after_update {
                        if qs.is_empty() {
                            qs.push('?');
                        } else {
                            qs.push('&');
                        }
                        qs.push_str(&format!("after_update={}", urlencoding::encode(&t)));
                    }
                    ("POST".into(), format!("/api/update/install{}", qs), None)
                }
            }
        }
        Commands::Mesh { action } => match action {
            MeshAction::Status => ("GET".into(), "/api/mesh/status".into(), None),
            MeshAction::Peers => ("GET".into(), "/api/mesh/peers".into(), None),
            MeshAction::Discover => ("POST".into(), "/api/mesh/discover".into(), None),
            MeshAction::Connect { instance_id } => (
                "POST".into(),
                "/api/mesh/connect".into(),
                Some(serde_json::json!({"instance_id": instance_id}).to_string()),
            ),
            MeshAction::Approve { request_id } => (
                "POST".into(),
                "/api/mesh/approve".into(),
                Some(serde_json::json!({"request_id": request_id}).to_string()),
            ),
            MeshAction::Reject { request_id } => (
                "POST".into(),
                "/api/mesh/reject".into(),
                Some(serde_json::json!({"request_id": request_id}).to_string()),
            ),
            MeshAction::Disconnect { instance_id } => (
                "POST".into(),
                "/api/mesh/disconnect".into(),
                Some(serde_json::json!({"instance_id": instance_id}).to_string()),
            ),
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
            // `peer set-addr` is local-only; map to local endpoint for clear errors.
            MeshAction::Peer { action } => match action {
                MeshPeerAction::SetAddr { peer, addr } => {
                    let (ip, port) = parse_ip_port(addr);
                    let mut body = serde_json::json!({"ip": ip});
                    if let Some(p) = port {
                        body["port"] = serde_json::json!(p);
                    }
                    (
                        "POST".into(),
                        format!("/api/mesh/peers/{}/addr", urlencoding::encode(peer)),
                        Some(body.to_string()),
                    )
                }
            },
            _ => ("GET".into(), "/api/mesh/status".into(), None),
        },
        Commands::Bridge { action } => match action {
            crate::BridgeAction::Restart => (
                "POST".into(),
                "/api/bridge/start".into(),
                Some(
                    serde_json::json!({
                        "service": "telegram",
                        "ensure_token": true,
                        "wait_for_lock_ms": 10000,
                    })
                    .to_string(),
                ),
            ),
            crate::BridgeAction::Send {
                text,
                from_pad,
                raw_text,
                to,
                append,
                ..
            } => (
                "POST".into(),
                "/api/bridge/send".into(),
                Some(
                    serde_json::json!({
                        "text": from_pad.as_ref().map(|path| crate::cmd::payload_source::read_from_pad(path)
                            .unwrap_or_else(|e| { eprintln!("Error: {e}"); std::process::exit(1) }))
                            .or_else(|| text.clone())
                            .map(|text| crate::cmd::bridge::bridge_text(text, *raw_text)),
                        "chat_id": to,
                        "append": append,
                    })
                    .to_string(),
                ),
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
            crate::SessionsAction::Search {
                query,
                provider,
                project,
                limit,
                max_snippets,
            } => {
                let mut path = format!("/api/sessions/search?q={}", urlencoding::encode(query));
                if let Some(p) = provider {
                    path.push_str(&format!("&provider={}", urlencoding::encode(p)));
                }
                if let Some(p) = project {
                    path.push_str(&format!("&project={}", urlencoding::encode(p)));
                }
                path.push_str(&format!("&limit={}&max_snippets={}", limit, max_snippets));
                ("GET".into(), path, None)
            }
            crate::SessionsAction::Resume {
                session_id,
                title,
                workspace_id,
                workspace_name,
                model,
            } => {
                let mut body = serde_json::Map::new();
                if let Some(t) = title {
                    body.insert("title".into(), serde_json::Value::String(t.clone()));
                }
                if let Some(w) = workspace_id {
                    body.insert("workspace_id".into(), serde_json::Value::String(w.clone()));
                }
                if let Some(n) = workspace_name {
                    body.insert(
                        "workspace_name".into(),
                        serde_json::Value::String(n.clone()),
                    );
                }
                if let Some(m) = model {
                    body.insert("model".into(), serde_json::Value::String(m.clone()));
                }
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
            crate::SessionsAction::Backups { action } => match action {
                crate::args::sessions::BackupsAction::List { .. } => {
                    ("GET".into(), "/api/session/backups".into(), None)
                }
                crate::args::sessions::BackupsAction::Restore { name } => (
                    "POST".into(),
                    format!("/api/session/backups/{}/restore", urlencoding::encode(name)),
                    None,
                ),
            },
        },
        Commands::Keepawake { action } => match action {
            Some(crate::KeepAwakeAction::Stop) => {
                ("POST".into(), "/api/keepawake/stop".into(), None)
            }
            Some(crate::KeepAwakeAction::Start {
                duration_secs,
                display,
                gnome_policy,
                system_block,
            }) => (
                "POST".into(),
                "/api/keepawake/start".into(),
                Some(
                    serde_json::json!({
                        "duration_secs": duration_secs,
                        "display": display,
                        "gnome_policy": gnome_policy,
                        "system_block": system_block,
                    })
                    .to_string(),
                ),
            ),
            _ => ("GET".into(), "/api/keepawake".into(), None),
        },
        Commands::Usage { action } => match action {
            crate::UsageAction::Latest {
                provider,
                account_id,
            } => {
                let mut path = format!(
                    "/api/usage/latest?provider={}",
                    urlencoding::encode(provider)
                );
                if let Some(a) = account_id {
                    path.push_str(&format!("&account_id={}", urlencoding::encode(a)));
                }
                ("GET".into(), path, None)
            }
            crate::UsageAction::Projection {
                provider,
                account_id,
            } => {
                let mut path = format!(
                    "/api/usage/projection?provider={}",
                    urlencoding::encode(provider)
                );
                if let Some(a) = account_id {
                    path.push_str(&format!("&account_id={}", urlencoding::encode(a)));
                }
                ("GET".into(), path, None)
            }
            crate::UsageAction::Refresh {
                provider,
                account_id,
            } => {
                let mut body = serde_json::json!({ "provider": provider });
                if let Some(a) = account_id {
                    body["account_id"] = serde_json::json!(a);
                }
                (
                    "POST".into(),
                    "/api/usage/refresh".into(),
                    Some(body.to_string()),
                )
            }
        },
        Commands::Service { action } => service_to_api(action),
        // Declared non-REST by the transport table, which answered before this
        // match was reached. Listed explicitly so a new variant must choose.
        Commands::Web { .. }
        | Commands::Plugin { .. }
        | Commands::Plan { .. }
        | Commands::Team { .. }
        | Commands::Spec { .. }
        | Commands::Effort { .. }
        | Commands::Orchestrator { .. }
        | Commands::Persona { .. }
        | Commands::Task { .. }
        | Commands::Inbox { .. }
        | Commands::Canvas { .. }
        | Commands::CliManifest { .. }
        | Commands::Capabilities
        | Commands::Pad { .. }
        | Commands::System { .. }
        | Commands::Docs { .. }
        | Commands::Debug { .. } => {
            unreachable!("transport table declares this command non-REST")
        }
    }
}
