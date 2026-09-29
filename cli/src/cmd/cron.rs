use crate::client::SocketClient;
use crate::CronAction;

fn print_cron_ack(json: bool, state: &str, id: &str) {
    if json {
        crate::cmd::print::print_json(&serde_json::json!({ "ok": true, "state": state, "id": id }));
    } else {
        let mut label = state.to_string();
        label[..1].make_ascii_uppercase();
        println!("{label}: {id}");
    }
}

pub fn handle_cron(client: &mut SocketClient, json: bool, action: CronAction) {
    match action {
        CronAction::List { workspace } => {
            let mut params = serde_json::json!({});
            if let Some(ws) = workspace {
                params["workspace_id"] = serde_json::json!(ws);
            }
            match client.call("cron.list", Some(params)) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() {
                            println!("No cron jobs.");
                        }
                        for j in arr {
                            let enabled =
                                j.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                            let oneshot =
                                j.get("oneshot").and_then(|v| v.as_bool()).unwrap_or(false);
                            let status = match (enabled, oneshot) {
                                (_, true) => "oneshot",
                                (true, false) => "enabled",
                                (false, false) => "disabled",
                            };
                            println!(
                                "{} {} [{}] schedule={} agent={}{}{}",
                                j.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                                j.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                                status,
                                j.get("schedule").and_then(|v| v.as_str()).unwrap_or("?"),
                                j.get("agent").and_then(|v| v.as_str()).unwrap_or("?"),
                                j.get("team_id")
                                    .and_then(|v| v.as_str())
                                    .map(|id| format!(" team={id}"))
                                    .unwrap_or_default(),
                                j.get("report_target_tab_id")
                                    .and_then(|v| v.as_str())
                                    .map(|id| format!(" reports_to={id}"))
                                    .unwrap_or_else(|| " private_worker".into()),
                            );
                        }
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        CronAction::Create {
            name,
            schedule,
            task,
            workspace,
            team,
            no_team,
            agent,
            model,
            reasoning_effort,
            cwd,
            skill,
            description,
            report_tab,
            once,
            jitter_max_ms,
            tz,
            timeout_secs,
        } => {
            let ws_id = workspace.unwrap_or_else(|| caller_workspace_id(client));
            let params = crate::args::cron::cron_create_body(
                &name,
                &schedule,
                &task,
                &ws_id,
                team.as_deref(),
                no_team,
                &agent,
                model.as_deref(),
                reasoning_effort.as_deref(),
                cwd.as_deref(),
                description.as_deref(),
                std::env::var("CODETERM_TAB_ID").ok().as_deref(),
                report_tab.as_deref(),
                once,
                jitter_max_ms,
                tz.as_deref(),
                timeout_secs,
                skill.as_deref(),
            );
            match client.call("cron.create", Some(params)) {
                Ok(val) => {
                    if json {
                        crate::cmd::print::print_json(&val);
                    } else {
                        println!(
                            "Created: {} ({})",
                            val.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                            val.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                        );
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        CronAction::Edit {
            id,
            name,
            schedule,
            task,
            cwd,
            skill,
            description,
            agent,
            model,
            reasoning_effort,
            jitter_max_ms,
            tz,
            timeout_secs,
        } => {
            let mut params = serde_json::json!({"id": id});
            if let Some(n) = name {
                params["name"] = serde_json::json!(n);
            }
            if let Some(s) = schedule {
                params["schedule"] = serde_json::json!(s);
            }
            if let Some(t) = task {
                params["task"] = serde_json::json!(t);
            }
            if let Some(c) = cwd {
                params["cwd"] = serde_json::json!(c);
            }
            if let Some(sk) = skill {
                params["skill"] = serde_json::json!(sk);
            }
            if let Some(d) = description {
                params["description"] = serde_json::json!(d);
            }
            if let Some(a) = agent {
                params["agent"] = serde_json::json!(a);
            }
            if let Some(m) = model {
                params["model"] = serde_json::json!(m);
            }
            if let Some(effort) = reasoning_effort {
                params["reasoning_effort"] = serde_json::json!(effort);
            }
            if let Some(j) = jitter_max_ms {
                params["jitter_max_ms"] = serde_json::json!(j);
            }
            if let Some(z) = tz {
                params["tz"] = serde_json::json!(z);
            }
            if let Some(ts) = timeout_secs {
                params["timeout_seconds"] = serde_json::json!(ts);
            }
            match client.call("cron.update", Some(params)) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        println!(
                            "Updated: {}",
                            val.get("name").and_then(|v| v.as_str()).unwrap_or("?")
                        );
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        CronAction::Delete { id } => {
            match client.call("cron.delete", Some(serde_json::json!({"id": id}))) {
                Ok(_) => print_cron_ack(json, "deleted", &id),
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        CronAction::Enable { id } => {
            match client.call("cron.enable", Some(serde_json::json!({"id": id}))) {
                Ok(_) => print_cron_ack(json, "enabled", &id),
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        CronAction::Disable { id } => {
            match client.call("cron.disable", Some(serde_json::json!({"id": id}))) {
                Ok(_) => print_cron_ack(json, "disabled", &id),
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        CronAction::Trigger { id } => {
            match client.call("cron.trigger", Some(serde_json::json!({"id": id}))) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        println!(
                            "Triggered: tab={}",
                            val.get("tab_id").and_then(|v| v.as_str()).unwrap_or("?")
                        );
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        CronAction::Cancel { id } => {
            match client.call("cron.cancel", Some(serde_json::json!({"id": id}))) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        let n = val.get("cancelled").and_then(|v| v.as_u64()).unwrap_or(0);
                        if n == 0 {
                            println!("No running executions for {}.", id);
                        } else {
                            println!("Cancelled {} running execution(s) of {}.", n, id);
                        }
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        CronAction::History { id } => {
            match client.call("cron.history", Some(serde_json::json!({"id": id}))) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() {
                            println!("No runs yet.");
                        }
                        for r in arr {
                            println!(
                                "{} {} started={} tab={}",
                                r.get("run_id").and_then(|v| v.as_str()).unwrap_or("?"),
                                r.get("status").and_then(|v| v.as_str()).unwrap_or("?"),
                                r.get("started_at").and_then(|v| v.as_u64()).unwrap_or(0),
                                r.get("tab_id").and_then(|v| v.as_str()).unwrap_or("-"),
                            );
                        }
                    }
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        CronAction::Complete { run_id, summary } => {
            let mut params = serde_json::json!({"run_id": run_id});
            if let Some(s) = summary {
                params["summary"] = serde_json::json!(s);
            }
            match client.call("cron.complete", Some(params)) {
                Ok(_) => print_cron_ack(json, "completed", &run_id),
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
    }
}

/// A scheduled job belongs to the workspace of the tab that created it. The
/// focused workspace is a last resort for a caller with no tab identity.
fn caller_workspace_id(client: &mut SocketClient) -> String {
    let workspaces = client
        .call("workspace.list", None)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let caller_tab_id = crate::nonempty_env("CODETERM_TAB_ID");
    let workspace_env = crate::nonempty_env("CODETERM_WORKSPACE_ID");
    crate::cmd::local_io::workspace_current_resolution(
        &workspaces,
        crate::cmd::local_io::WorkspaceIdentity {
            explicit: None,
            caller_tab_id: caller_tab_id.as_deref(),
            workspace_env: workspace_env.as_deref(),
        },
    )
    .map(|resolved| resolved.id.to_string())
    .unwrap_or_default()
}
