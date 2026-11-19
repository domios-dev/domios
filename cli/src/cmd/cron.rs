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

fn cron_completion_params(
    run_id: &str,
    summary: Option<&str>,
    caller_tab_id: Option<&str>,
    binding_nonce: Option<&str>,
    human_confirmed: bool,
) -> serde_json::Value {
    let mut params = serde_json::json!({
        "run_id": run_id,
        "human_confirmed": human_confirmed,
    });
    if let Some(summary) = summary {
        params["summary"] = serde_json::json!(summary);
    }
    if let Some(caller_tab_id) = caller_tab_id {
        params["caller_tab_id"] = serde_json::json!(caller_tab_id);
        if let Some(binding_nonce) = binding_nonce {
            params["session_binding_nonce"] = serde_json::json!(binding_nonce);
        }
    }
    params
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
                        println!("{}", crate::cmd::output::encode(&val).unwrap());
                    } else if !crate::cmd::output::verbose() {
                        print!("{}", crate::cmd::output::compact_list(&val, None));
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
                crate::nonempty_env("CODETERM_TAB_ID").as_deref(),
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
        action @ CronAction::Edit { .. } => {
            let (_, params) = action.update_request().expect("cron edit request");
            match client.call("cron.update", Some(params)) {
                Ok(val) => {
                    if json {
                        println!("{}", crate::cmd::output::encode(&val).unwrap());
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
                        println!("{}", crate::cmd::output::encode(&val).unwrap());
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
                        println!("{}", crate::cmd::output::encode(&val).unwrap());
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
                        println!("{}", crate::cmd::output::encode(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() {
                            println!("No runs yet.");
                        }
                        for r in arr {
                            let time = |field: &str| r.get(field).and_then(|value| value.as_u64());
                            let scheduled = time("scheduled_occurrence_at");
                            let fired = time("fired_at");
                            let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                            let outcome = match (status, scheduled, fired, time("late_by_ms")) {
                                ("Cancelled", Some(_), None, _) => "Missed".to_string(),
                                (_, _, _, Some(delay)) => format!("{status} late_by_ms={delay}"),
                                _ => status.to_string(),
                            };
                            let timestamp = |value: Option<u64>| {
                                value.map_or_else(|| "-".into(), |value| value.to_string())
                            };
                            println!(
                                "{} {} scheduled={} fired={} started={} tab={}",
                                r.get("run_id").and_then(|v| v.as_str()).unwrap_or("?"),
                                outcome,
                                timestamp(scheduled),
                                timestamp(fired),
                                timestamp(time("started_at")),
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
        CronAction::Complete {
            run_id,
            summary,
            yes,
        } => {
            let caller_tab_id = crate::nonempty_env("CODETERM_TAB_ID");
            let binding_nonce = crate::nonempty_env("CODETERM_SESSION_BINDING_NONCE");
            let params = cron_completion_params(
                &run_id,
                summary.as_deref(),
                caller_tab_id.as_deref(),
                binding_nonce.as_deref(),
                yes,
            );
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

#[cfg(test)]
mod tests {
    use super::cron_completion_params;

    #[test]
    fn cron_complete_carries_the_current_tab_binding() {
        assert_eq!(
            cron_completion_params(
                "run-token",
                Some("summary-token"),
                Some("tab-token"),
                Some("nonce-token"),
                false,
            ),
            serde_json::json!({
                "run_id": "run-token",
                "summary": "summary-token",
                "caller_tab_id": "tab-token",
                "session_binding_nonce": "nonce-token",
                "human_confirmed": false,
            })
        );
    }

    #[test]
    fn cron_complete_without_tab_identity_is_unconfirmed_by_default() {
        assert_eq!(
            cron_completion_params("run-token", None, None, Some("unused"), false),
            serde_json::json!({ "run_id": "run-token", "human_confirmed": false })
        );
    }

    #[test]
    fn cron_complete_carries_explicit_human_confirmation() {
        assert_eq!(
            cron_completion_params("run-token", None, None, None, true),
            serde_json::json!({ "run_id": "run-token", "human_confirmed": true })
        );
    }
}
