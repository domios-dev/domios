use crate::{
    cmd::{delayed::parse_schedule, send_text::resolve_send_text_input},
    Commands, UpdateAction,
};

use super::update::resolve_update_action;
use super::{parse_ip_port, MeshAction, MeshPeerAction};

mod actions;

pub(crate) use actions::send_text_to_api;
use actions::{delayed_create_to_api, mem_to_api, now_ms, service_to_api};

pub const UNSUPPORTED_OVER_MESH: &str = "UNSUPPORTED";

fn unsupported(reason: &str) -> (String, String, Option<String>) {
    (UNSUPPORTED_OVER_MESH.into(), reason.into(), None)
}

pub fn cli_command_to_api(cmd: &Commands) -> (String, String, Option<String>) {
    match cmd {
        Commands::Web { .. } => unsupported(
            "web link is local-only — each peer's link needs that peer's own token; run it there",
        ),
        Commands::Plugin { .. } => {
            unsupported("plugin management is local-only — run it on the target machine")
        }
        Commands::Plan { .. } => {
            unsupported("plan management is local-only — run it on the target machine")
        }
        Commands::Team { .. } => {
            unsupported("team management is local-only — run it on the target machine")
        }
        Commands::Spec { .. } => {
            unsupported("spec management is local-only — run it on the target machine")
        }
        Commands::Effort { .. } => {
            unsupported("effort management is local-only — run it on the target machine")
        }
        Commands::Orchestrator { .. } => {
            unsupported("orchestrator handoff is local-only — run it on the target machine")
        }
        Commands::Orchestration { action } => match action {
            crate::args::orchestration::OrchestrationAction::New {
                orchestrator,
                planner,
                workspace,
                title,
            } => {
                let (orchestrator_agent, orchestrator_model) =
                    crate::cmd::orchestration::split_provider_model(orchestrator);
                let (planner_agent, planner_model) =
                    crate::cmd::orchestration::split_provider_model(planner);
                let mut body = serde_json::json!({
                    "orchestrator_agent": orchestrator_agent,
                    "planner_agent": planner_agent,
                });
                if let Some(m) = orchestrator_model {
                    body["orchestrator_model"] = serde_json::json!(m);
                }
                if let Some(m) = planner_model {
                    body["planner_model"] = serde_json::json!(m);
                }
                if let Some(w) = workspace {
                    body["workspace_id"] = serde_json::json!(w);
                }
                if let Some(t) = title {
                    body["title"] = serde_json::json!(t);
                }
                (
                    "POST".into(),
                    "/api/orchestration/new".into(),
                    Some(body.to_string()),
                )
            }
        },
        Commands::Task { .. } => {
            unsupported("task management is local-only — run it on the target machine")
        }
        Commands::Canvas { .. } => {
            unsupported("canvas control is local-only — run it on the target machine")
        }
        // `cli-manifest` introspects the local clap binary — no remote form.
        Commands::CliManifest { .. } => {
            unsupported("cli-manifest is local-only — it dumps this binary's own command tree")
        }
        Commands::Identify => ("GET".into(), "/api/identify".into(), None),
        Commands::Capabilities => {
            unsupported("capabilities is socket-only; run it on the target machine")
        }
        Commands::SendText {
            text,
            pane,
            from_file,
            stdin: use_stdin,
            actions,
            delay,
            at,
        } => {
            let mut stdin = std::io::stdin();
            if delay.is_some() || at.is_some() {
                delayed_create_to_api(text, pane, from_file, *use_stdin, delay, at, &mut stdin)
            } else {
                send_text_to_api(text, pane, from_file, *use_stdin, actions, &mut stdin)
            }
        }
        Commands::Delayed { action } => match action {
            crate::DelayedAction::List { pane } => {
                let p = pane.as_deref().unwrap_or("general");
                (
                    "GET".into(),
                    format!("/api/panes/{}/delayed-messages", p),
                    None,
                )
            }
            crate::DelayedAction::Create {
                text,
                pane,
                delay,
                at,
                from_file,
                stdin: use_stdin,
            } => {
                let mut stdin = std::io::stdin();
                delayed_create_to_api(text, pane, from_file, *use_stdin, delay, at, &mut stdin)
            }
            crate::DelayedAction::Edit {
                id,
                text,
                delay,
                at,
                from_file,
                stdin: use_stdin,
            } => {
                let mut body = serde_json::Map::new();
                if text.is_some() || from_file.is_some() || *use_stdin {
                    match resolve_send_text_input(
                        text.clone(),
                        from_file.clone(),
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
        Commands::Notify { title, body, level } => (
            "POST".into(),
            "/api/notifications".into(),
            Some(serde_json::json!({"title": title, "body": body, "level": level}).to_string()),
        ),
        Commands::Workspace { action } => match action {
            crate::WorkspaceAction::List => ("GET".into(), "/api/workspaces".into(), None),
            crate::WorkspaceAction::Create { name, .. } => (
                "POST".into(),
                "/api/workspaces".into(),
                Some(serde_json::json!({"name": name}).to_string()),
            ),
            crate::WorkspaceAction::Focus { id } => {
                ("POST".into(), format!("/api/workspaces/{}/focus", id), None)
            }
            crate::WorkspaceAction::Rename { id, name } => (
                "POST".into(),
                format!("/api/workspaces/{}/rename", id),
                Some(serde_json::json!({"name": name}).to_string()),
            ),
            crate::WorkspaceAction::Close { id } => {
                ("POST".into(), format!("/api/workspaces/{}/close", id), None)
            }
            _ => ("GET".into(), "/api/workspaces".into(), None),
        },
        Commands::Tab { action } => match action {
            crate::PaneAction::List { .. } => ("GET".into(), "/api/panes".into(), None),
            crate::PaneAction::Current { .. } => ("GET".into(), "/api/panes".into(), None),
            crate::PaneAction::New {
                workspace,
                cwd,
                title,
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
                ("POST".into(), "/api/panes".into(), Some(body.to_string()))
            }
            crate::PaneAction::Focus { id } => {
                ("POST".into(), format!("/api/panes/{}/focus", id), None)
            }
            crate::PaneAction::Close { id } => {
                ("POST".into(), format!("/api/panes/{}/close", id), None)
            }
            crate::PaneAction::Rename { title, pane } => {
                let p = pane.as_deref().unwrap_or("general");
                (
                    "POST".into(),
                    format!("/api/panes/{}/rename", p),
                    Some(serde_json::json!({"title": title}).to_string()),
                )
            }
            crate::PaneAction::Describe { summary, pane } => {
                let p = pane.as_deref().unwrap_or("general");
                let description = match crate::cmd::agent::normalize_pane_description(summary) {
                    Ok(Some(d)) => d,
                    Ok(None) => String::new(),
                    Err(e) => return unsupported(&e),
                };
                (
                    "POST".into(),
                    format!("/api/panes/{}/describe", p),
                    Some(serde_json::json!({"description": description}).to_string()),
                )
            }
            crate::PaneAction::GetText { pane, formatted } => {
                let p = pane.as_deref().unwrap_or("general");
                (
                    "GET".into(),
                    format!("/api/panes/{}/text?formatted={}", p, formatted),
                    None,
                )
            }
            crate::PaneAction::Exec {
                command,
                pane,
                timeout,
            } => {
                let p = pane.as_deref().unwrap_or("general");
                (
                    "POST".into(),
                    format!("/api/panes/{}/exec", p),
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
            crate::PaneAction::Status { pane } => {
                let p = pane.as_deref().unwrap_or("general");
                ("GET".into(), format!("/api/panes/{}/status", p), None)
            }
            crate::PaneAction::Search { .. } => {
                // pane search runs local CLI-side; peer only supplies raw list.
                // Local ranking keeps `pane list` output stable.
                ("GET".into(), "/api/panes".into(), None)
            }
            crate::PaneAction::Stop { pane } => {
                let p = pane.as_deref().unwrap_or("general");
                (
                    "POST".into(),
                    format!("/api/panes/{}/stop", p),
                    Some("{}".into()),
                )
            }
            crate::PaneAction::Chat { pane, since } => {
                let p = pane.as_deref().unwrap_or("general");
                (
                    "GET".into(),
                    format!("/api/panes/{}/chat?since={}", p, since),
                    None,
                )
            }
            crate::PaneAction::Capture { action } => match action {
                crate::args::misc::PaneCaptureAction::Start { pane, out } => {
                    let p = pane.as_deref().unwrap_or("general");
                    (
                        "POST".into(),
                        format!("/api/panes/{}/capture/start", p),
                        Some(serde_json::json!({ "out": out }).to_string()),
                    )
                }
                crate::args::misc::PaneCaptureAction::Stop { pane } => {
                    let p = pane.as_deref().unwrap_or("general");
                    (
                        "POST".into(),
                        format!("/api/panes/{}/capture/stop", p),
                        None,
                    )
                }
            },
        },
        Commands::Paste { action } => match action {
            crate::PasteAction::Put {
                body,
                from_file,
                from_stdin,
                ttl,
                content_type,
                pop,
            } => {
                let resolved = if *from_stdin {
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
                    body.clone()
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
                let q = if *pop { "?pop=true" } else { "" };
                ("GET".into(), format!("/api/pasteboard/{}{}", id, q), None)
            }
            crate::PasteAction::List => ("GET".into(), "/api/pasteboard".into(), None),
            crate::PasteAction::Rm { id } => {
                ("DELETE".into(), format!("/api/pasteboard/{}", id), None)
            }
        },
        Commands::Report(_) => ("GET".into(), "/api/panes".into(), None),
        Commands::Agent { action } => match action {
            crate::AgentAction::Spawn {
                agent,
                title,
                description,
                task,
                task_file,
                role,
                parent,
                plan,
                report_to,
                workspace,
                can_spawn,
                cwd,
                args,
                model,
                reasoning_effort,
                system_prompt,
                preset,
                charter,
                charter_file,
                interval,
                nudge_cooldown,
                max_actions,
                allow_shared_checkout,
                bound_task,
                plan_task,
            } => {
                let task = match crate::cmd::agent::resolve_spawn_task(
                    task.clone(),
                    task_file.clone(),
                    &mut std::io::stdin(),
                ) {
                    Ok(t) => t,
                    Err(e) => return unsupported(&format!("agent spawn task input error: {}", e)),
                };
                let charter = match crate::cmd::agent::resolve_spawn_charter(
                    charter.clone(),
                    charter_file.clone(),
                    &mut std::io::stdin(),
                ) {
                    Ok(t) => t,
                    Err(e) => {
                        return unsupported(&format!("agent spawn charter input error: {}", e))
                    }
                };
                let agent_name = agent.as_str();
                let title = crate::cmd::agent::join_cli_words(title.clone());
                let mut body = serde_json::json!({
                    "agent": agent_name,
                    "title": title,
                    "task": task,
                });
                if let Some(d) = crate::cmd::agent::join_cli_words(description.clone()) {
                    match crate::cmd::agent::normalize_pane_description(&d) {
                        Ok(Some(d)) => body["description"] = serde_json::json!(d),
                        Ok(None) => body["description"] = serde_json::json!(""),
                        Err(e) => return unsupported(&e),
                    }
                }
                if let Some(r) = role {
                    body["role"] = serde_json::json!(r.as_str());
                    if let Some(profile) = r.role_profile() {
                        body["role_profile"] = serde_json::json!(profile);
                    }
                }
                if let Some(p) = parent {
                    body["parent_pane_id"] = serde_json::json!(p);
                }
                if let Some(pl) = plan {
                    body["plan_id"] = serde_json::json!(pl);
                }
                let report_to = report_to.clone().or_else(|| {
                    std::env::var("CODETERM_PANE_ID")
                        .ok()
                        .filter(|s| !s.is_empty())
                });
                if let Some(rt) = report_to {
                    body["report_to"] = serde_json::json!(rt);
                }
                if let Some(w) = workspace {
                    body["workspace_id"] = serde_json::json!(w);
                }
                if *can_spawn {
                    body["can_spawn"] = serde_json::json!(true);
                }
                if let Some(c) = cwd {
                    body["cwd"] = serde_json::json!(c);
                }
                if let Some(a) = args {
                    if !a.is_empty() {
                        body["args"] = serde_json::json!(a);
                    }
                }
                if let Some(m) = model {
                    body["model"] = serde_json::json!(m);
                }
                if let Some(effort) = reasoning_effort {
                    body["reasoning_effort"] = serde_json::json!(effort);
                }
                if let Some(system_prompt) = system_prompt {
                    body["system_prompt"] = serde_json::json!(system_prompt);
                }
                if let Some(preset) = preset {
                    body["preset"] = serde_json::json!(preset);
                }
                if let Some(charter) = charter {
                    body["charter"] = serde_json::json!(charter);
                }
                if let Some(interval) = interval {
                    let secs = match crate::cmd::agent::parse_duration_secs(interval, "--interval")
                    {
                        Ok(v) => v,
                        Err(e) => return unsupported(&e),
                    };
                    body["tick_interval_secs"] = serde_json::json!(secs);
                }
                if let Some(nudge_cooldown) = nudge_cooldown {
                    let secs = match crate::cmd::agent::parse_duration_secs(
                        nudge_cooldown,
                        "--nudge-cooldown",
                    ) {
                        Ok(v) => v,
                        Err(e) => return unsupported(&e),
                    };
                    body["nudge_cooldown_secs"] = serde_json::json!(secs);
                }
                if let Some(max_actions) = max_actions {
                    body["max_actions_per_tick"] = serde_json::json!(max_actions);
                }
                if *allow_shared_checkout {
                    body["allow_shared_checkout"] = serde_json::json!(true);
                }
                if let Some(tid) = bound_task {
                    body["task_id"] = serde_json::json!(tid);
                }
                if let Some(pt) = plan_task {
                    body["plan_task"] = serde_json::json!(pt);
                }
                (
                    "POST".into(),
                    "/api/agents/spawn".into(),
                    Some(body.to_string()),
                )
            }
            crate::AgentAction::Providers | crate::AgentAction::Models { .. } => {
                ("GET".into(), "/api/agents/providers".into(), None)
            }
            _ => ("GET".into(), "/api/panes".into(), None),
        },
        Commands::Notifications { unread, .. } => {
            let qs = if *unread { "?unread=true" } else { "" };
            ("GET".into(), format!("/api/notifications{}", qs), None)
        }
        Commands::Cron { action } => match action {
            crate::CronAction::List { .. } => ("GET".into(), "/api/crons".into(), None),
            crate::CronAction::Create {
                name,
                schedule,
                task,
                workspace,
                agent,
                model,
                cwd,
                skill,
                description,
                creator_pane,
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
                    agent,
                    model.as_deref(),
                    cwd.as_deref(),
                    description.as_deref(),
                    creator_pane.as_deref(),
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
            crate::ConfigAction::Get { key } => {
                ("GET".into(), format!("/api/config?key={}", key), None)
            }
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
                    after_update,
                } => {
                    if from_file.is_some() || continue_prompt.is_some() || continue_tab.is_some() {
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
                raw_text,
                to,
                append,
                ..
            } => (
                "POST".into(),
                "/api/bridge/send".into(),
                Some(
                    serde_json::json!({
                        "text": text
                            .as_ref()
                            .map(|text| crate::cmd::bridge::bridge_text(text.clone(), *raw_text)),
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
        // `system` is local-only platform config (schtasks/launchd/systemd).
        // Mesh relay falls through to /identify for a valid response shape.
        Commands::System { .. } => ("GET".into(), "/api/identify".into(), None),
        // `docs` / `doc` read embedded markdown — local-only by definition.
        Commands::Docs { .. } => ("GET".into(), "/api/identify".into(), None),
        Commands::Doc { .. } => ("GET".into(), "/api/identify".into(), None),
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
        Commands::Debug { .. } => {
            eprintln!("Error: `debug` is local-only — refusing to crash a remote peer via --mesh");
            std::process::exit(2);
        }
    }
}
