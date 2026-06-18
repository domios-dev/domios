use crate::client::SocketClient;
use crate::cmd::print::{handle_result, print_object};
use crate::{build_http, read_api_config, resolve_pane, AgentAction};

fn format_agent_models(target: &serde_json::Value, provider: &str) -> String {
    let models = target.get("available_models").and_then(|v| v.as_array());
    let Some(models) = models else {
        return format!("No models advertised for provider '{}'.\n", provider);
    };
    let default_model = target.get("default_model").and_then(|v| v.as_str());
    let mut out = String::new();
    for m in models {
        let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let name = m.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
        let marker = if default_model == Some(id) {
            " (default)"
        } else {
            ""
        };
        let description = m.get("description").and_then(|v| v.as_str()).unwrap_or("");
        if description.is_empty() {
            out.push_str(&format!("{:<10} {}{}\n", id, name, marker));
        } else {
            out.push_str(&format!(
                "{:<10} {}{}\n             {}\n",
                id, name, marker, description
            ));
        }
    }
    out
}

pub fn handle_agent(client: &mut SocketClient, json: bool, action: AgentAction) {
    let (base, token) = read_api_config();
    let http = match build_http(&token) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to build HTTP client: {}", e);
            std::process::exit(1);
        }
    };
    match action {
        AgentAction::Providers => match http.get(format!("{}/api/agents/providers", base)).send() {
            Ok(resp) => {
                let status = resp.status();
                let val: serde_json::Value = resp.json().unwrap_or_default();
                if json {
                    println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    return;
                }
                let Some(arr) = val.as_array() else {
                    if status == 404 {
                        eprintln!(
                                "Server doesn't know /api/agents/providers — update the running CodeTerm instance."
                            );
                    } else {
                        eprintln!("Unexpected response shape (HTTP {})", status);
                    }
                    std::process::exit(1);
                };
                if arr.is_empty() {
                    println!("No agent providers registered.");
                    return;
                }
                for p in arr {
                    let kind = p.get("kind").and_then(|v| v.as_str()).unwrap_or("agent");
                    let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    let name = p.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
                    let default_model = p
                        .get("default_model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let models_count = p
                        .get("available_models")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    println!(
                        "{:<12} {:<6} {:<20} default={}  models={}",
                        id, kind, name, default_model, models_count
                    );
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
        AgentAction::Models { provider } => {
            match http.get(format!("{}/api/agents/providers", base)).send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    let Some(arr) = val.as_array() else {
                        eprintln!("Unexpected response shape");
                        std::process::exit(1);
                    };
                    let target = arr
                        .iter()
                        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(provider.as_str()));
                    let Some(target) = target else {
                        let known: Vec<&str> = arr
                            .iter()
                            .filter_map(|p| p.get("id").and_then(|v| v.as_str()))
                            .collect();
                        eprintln!(
                            "Unknown provider: '{}'. Known: [{}]",
                            provider,
                            known.join(", ")
                        );
                        std::process::exit(1);
                    };
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(
                                target
                                    .get("available_models")
                                    .unwrap_or(&serde_json::Value::Null)
                            )
                            .unwrap()
                        );
                        return;
                    }
                    print!("{}", format_agent_models(target, &provider));
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::SetOrchestrator { pane } => {
            let pane_id = resolve_pane(client, pane);
            match http
                .post(format!("{}/api/panes/{}/role", base, pane_id))
                .json(&serde_json::json!({"role": "Orchestrator"}))
                .send()
            {
                Ok(resp) => {
                    let v: serde_json::Value = resp.json().unwrap_or_default();
                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Link { parent, pane } => {
            let pane_id = resolve_pane(client, pane);
            let parent_resp = http
                .get(format!("{}/api/panes/{}/status", base, parent))
                .send();
            let role = match parent_resp {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    match val.get("role").and_then(|v| v.as_str()) {
                        Some("Orchestrator") | Some("Conductor") => "Worker",
                        _ => {
                            eprintln!("Parent must be an Orchestrator");
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            match http
                .post(format!("{}/api/panes/{}/role", base, pane_id))
                .json(&serde_json::json!({"role": role, "parent_pane_id": parent}))
                .send()
            {
                Ok(resp) => {
                    let v: serde_json::Value = resp.json().unwrap_or_default();
                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Unlink { pane } => {
            let pane_id = resolve_pane(client, pane);
            match http
                .delete(format!("{}/api/panes/{}/role", base, pane_id))
                .send()
            {
                Ok(resp) => {
                    let v: serde_json::Value = resp.json().unwrap_or_default();
                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::AllowSpawn { pane, enable } => {
            let pane_id = resolve_pane(client, pane);
            match http
                .post(format!("{}/api/panes/{}/can-spawn", base, pane_id))
                .json(&serde_json::json!({"can_spawn": enable}))
                .send()
            {
                Ok(resp) => {
                    let v: serde_json::Value = resp.json().unwrap_or_default();
                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Report { message, pane } => {
            let pane_id = resolve_pane(client, pane);
            let result = client.call(
                "agent.report",
                Some(serde_json::json!({
                    "pane_id": pane_id,
                    "message": message,
                })),
            );
            handle_result(json, result);
        }
        AgentAction::Reports { pane, clear, limit } => {
            let pane_id = resolve_pane(client, pane);
            match client.call(
                "agent.reports",
                Some(serde_json::json!({
                    "pane_id": pane_id,
                    "clear": clear,
                })),
            ) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() {
                            println!("No reports.");
                        }
                        let take = limit.unwrap_or(arr.len());
                        for r in arr.iter().rev().take(take) {
                            let from = r.get("from_title").and_then(|v| v.as_str()).unwrap_or("?");
                            let msg = r.get("message").and_then(|v| v.as_str()).unwrap_or("");
                            println!("[{}] {}", from, msg);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Spawn {
            agent,
            title,
            cwd,
            role,
            parent,
            can_spawn,
            task,
            args,
            workspace,
            model,
            system_prompt,
            preset,
        } => {
            let mut body = serde_json::json!({
                "agent": agent,
            });
            if let Some(t) = title {
                body["title"] = serde_json::json!(t);
            }
            if let Some(c) = cwd {
                body["cwd"] = serde_json::json!(c);
            }
            if let Some(r) = role {
                body["role"] = serde_json::json!(r.as_str());
            }
            if let Some(p) = parent {
                body["parent_pane_id"] = serde_json::json!(p);
            }
            if can_spawn {
                body["can_spawn"] = serde_json::json!(true);
            }
            if let Some(t) = task {
                body["task"] = serde_json::json!(t);
            }
            if let Some(a) = args {
                body["args"] = serde_json::json!(a);
            }
            if let Some(w) = workspace {
                body["workspace_id"] = serde_json::json!(w);
            }
            if let Some(m) = model {
                body["model"] = serde_json::json!(m);
            }
            if let Some(system_prompt) = system_prompt {
                body["system_prompt"] = serde_json::json!(system_prompt);
            }
            if let Some(preset) = preset {
                body["preset"] = serde_json::json!(preset);
            }
            // Record the spawner's pane for report-routing fallback.
            if let Ok(spawner) = std::env::var("CODETERM_PANE_ID") {
                if !spawner.is_empty() {
                    body["spawned_by_pane_id"] = serde_json::json!(spawner);
                }
            }
            match client.call("agent.spawn", Some(body)) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                        return;
                    }
                    let pid = val.get("pane_id").and_then(|v| v.as_str());
                    let title_str = val.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                    let warning = val.get("warning").and_then(|v| v.as_str());
                    if let Some(pid) = pid {
                        println!("Spawned: {} (pane {})", title_str, pid);
                    } else if let Some(err) = val.get("error").or_else(|| val.get("message")) {
                        eprintln!("Error: {}", err);
                    } else {
                        print_object(&val);
                    }
                    if let Some(warning) = warning {
                        eprintln!("Warning: {}", warning);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Compact { pane } => {
            let pane_id = resolve_pane(client, pane);
            let cmd = "/compact\n";
            match http
                .post(format!("{}/api/panes/{}/send", base, pane_id))
                .json(&serde_json::json!({"text": cmd}))
                .send()
            {
                Ok(_) => {
                    println!("Sent /compact to {}", pane_id);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Restart {
            pane,
            force,
            grace_secs,
        } => {
            let pane_id = resolve_pane(client, pane);
            let body = serde_json::json!({"force": force, "grace_secs": grace_secs});
            match http
                .post(format!("{}/api/panes/{}/agent/restart", base, pane_id))
                .json(&body)
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let retry = resp
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("30");
                        eprintln!("Error: rate limited (retry after {}s)", retry);
                        std::process::exit(1);
                    }
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if !status.is_success() {
                        let msg = val
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or_else(|| val.as_str().unwrap_or("restart failed"));
                        eprintln!("Error: {}", msg);
                        std::process::exit(1);
                    }
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        println!("Restarting pane {}...", pane_id);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        AgentAction::Model { name, pane } => {
            let pane_id = resolve_pane(client, pane);
            let agent_type = http
                .get(format!("{}/api/panes", base))
                .send()
                .ok()
                .and_then(|r| r.json::<serde_json::Value>().ok())
                .and_then(|v| {
                    v.as_array()
                        .map(|arr| {
                            arr.iter()
                                .find(|p| p.get("id").and_then(|i| i.as_str()) == Some(&pane_id))
                                .and_then(|p| {
                                    p.get("agent_type")
                                        .and_then(|t| t.as_str())
                                        .map(String::from)
                                })
                        })
                        .flatten()
                });
            if matches!(agent_type.as_deref(), Some("codex")) {
                eprintln!("Error: Codex /model is an interactive picker, not scriptable.");
                eprintln!("hint: kill the pane and respawn with the model:");
                eprintln!("      codeterm agent spawn codex --model {}", name);
                std::process::exit(1);
            }
            // raw: bypass bracket paste so the TUI parses /model as a slash command, not chat input.
            let cmd = format!("/model {}", name);
            match http
                .post(format!("{}/api/panes/{}/send", base, pane_id))
                .json(&serde_json::json!({"text": cmd, "raw": true}))
                .send()
            {
                Ok(_) => {
                    println!("Sent /model {} to {}", name, pane_id);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_models_formats_descriptor_models_from_api_snapshot() {
        let provider = serde_json::json!({
            "id": "codex",
            "default_model": "live-codex",
            "available_models": [
                {
                    "id": "live-codex",
                    "display_name": "Live Codex",
                    "description": "catalog snapshot"
                }
            ]
        });

        let out = format_agent_models(&provider, "codex");

        assert!(out.contains("live-codex"));
        assert!(out.contains("Live Codex (default)"));
        assert!(out.contains("catalog snapshot"));
        assert!(!out.contains("static-codex"));
    }
}
