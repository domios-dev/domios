use crate::cmd::print::{handle_result, print_object};
use crate::{build_http, read_api_config, resolve_pane, AgentAction};
use crate::client::SocketClient;

pub fn handle_agent(client: &mut SocketClient, json: bool, action: AgentAction) {
    let (base, token) = read_api_config();
    let http = build_http(&token);
    match action {
        AgentAction::List => {
            match http.get(format!("{}/api/agents", base)).send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() { println!("No agents found."); }
                        for a in arr { print_object(a); println!(); }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Providers => {
            match http.get(format!("{}/api/agents/providers", base)).send() {
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
                        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                        let name = p.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
                        let default_model = p.get("default_model").and_then(|v| v.as_str()).unwrap_or("-");
                        let models_count = p
                            .get("available_models")
                            .and_then(|v| v.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);
                        println!("{:<12} {:<20} default={}  models={}", id, name, default_model, models_count);
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Models { provider } => {
            match http.get(format!("{}/api/agents/providers", base)).send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    let Some(arr) = val.as_array() else {
                        eprintln!("Unexpected response shape");
                        std::process::exit(1);
                    };
                    let target = arr.iter().find(|p| {
                        p.get("id").and_then(|v| v.as_str()) == Some(provider.as_str())
                    });
                    let Some(target) = target else {
                        let known: Vec<&str> = arr.iter()
                            .filter_map(|p| p.get("id").and_then(|v| v.as_str()))
                            .collect();
                        eprintln!(
                            "Unknown provider: '{}'. Known: [{}]",
                            provider,
                            known.join(", ")
                        );
                        std::process::exit(1);
                    };
                    let models = target.get("available_models").and_then(|v| v.as_array());
                    if json {
                        println!("{}", serde_json::to_string_pretty(&models).unwrap());
                        return;
                    }
                    let Some(models) = models else {
                        println!("No models advertised for provider '{}'.", provider);
                        return;
                    };
                    let default_model = target.get("default_model").and_then(|v| v.as_str());
                    for m in models {
                        let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                        let name = m.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
                        let marker = if default_model == Some(id) { " (default)" } else { "" };
                        let description = m.get("description").and_then(|v| v.as_str()).unwrap_or("");
                        if description.is_empty() {
                            println!("{:<10} {}{}", id, name, marker);
                        } else {
                            println!("{:<10} {}{}\n             {}", id, name, marker, description);
                        }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Hierarchy => {
            match http.get(format!("{}/api/agents/hierarchy", base)).send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        fn print_tree(node: &serde_json::Value, indent: usize) {
                            let prefix = "  ".repeat(indent);
                            let role = node.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                            let title = node.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                            let status = node.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                            let icon = match role {
                                "Orchestrator" => "★",
                                "Lead" => "·",
                                "Worker" => "⚙",
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
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::SetConductor { pane } => {
            let pane_id = resolve_pane(client, pane);
            match http.post(format!("{}/api/panes/{}/role", base, pane_id))
                .json(&serde_json::json!({"role": "Orchestrator"}))
                .send()
            {
                Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Link { parent, pane } => {
            let pane_id = resolve_pane(client, pane);
            let parent_resp = http.get(format!("{}/api/panes/{}/status", base, parent)).send();
            let role = match parent_resp {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    match val.get("role").and_then(|v| v.as_str()) {
                        Some("Orchestrator") | Some("Conductor") => "Worker",
                        Some("Lead") | Some("Worker") => "Worker",
                        _ => { eprintln!("Parent must be Orchestrator or Lead"); std::process::exit(1); }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            };
            match http.post(format!("{}/api/panes/{}/role", base, pane_id))
                .json(&serde_json::json!({"role": role, "parent_pane_id": parent}))
                .send()
            {
                Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Unlink { pane } => {
            let pane_id = resolve_pane(client, pane);
            match http.delete(format!("{}/api/panes/{}/role", base, pane_id)).send() {
                Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Children { pane } => {
            let pane_id = resolve_pane(client, pane);
            match http.get(format!("{}/api/panes/{}/children", base, pane_id)).send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() { println!("No children."); }
                        for c in arr { print_object(c); println!(); }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::AllowSpawn { pane, enable } => {
            let pane_id = resolve_pane(client, pane);
            match http.post(format!("{}/api/panes/{}/can-spawn", base, pane_id))
                .json(&serde_json::json!({"can_spawn": enable}))
                .send()
            {
                Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Report { message, pane } => {
            let pane_id = resolve_pane(client, pane);
            let result = client.call("agent.report", Some(serde_json::json!({
                "pane_id": pane_id,
                "message": message,
            })));
            handle_result(json, result);
        }
        AgentAction::Reports { pane, clear, limit } => {
            let pane_id = resolve_pane(client, pane);
            match client.call("agent.reports", Some(serde_json::json!({
                "pane_id": pane_id,
                "clear": clear,
            }))) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() { println!("No reports."); }
                        let take = limit.unwrap_or(arr.len());
                        for r in arr.iter().rev().take(take) {
                            let from = r.get("from_title").and_then(|v| v.as_str()).unwrap_or("?");
                            let msg = r.get("message").and_then(|v| v.as_str()).unwrap_or("");
                            println!("[{}] {}", from, msg);
                        }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Status { pane } => {
            let pane_id = resolve_pane(client, pane);
            match http.get(format!("{}/api/panes/{}/status", base, pane_id)).send() {
                Ok(resp) => {
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        print_object(&val);
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Spawn { agent, title, cwd, role, parent, can_spawn, task, args, workspace, model, no_wait } => {
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
            if let Some(m) = model { body["model"] = serde_json::json!(m); }
            // Always record the spawning agent's pane (when this CLI runs
            // inside a CodeTerm PTY). Independent of --parent, so a
            // top-level Orchestrator (no parent in the role hierarchy)
            // still has a route back for `agent report`.
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
                    let ready = val.get("ready").and_then(|v| v.as_bool());
                    let ready_error = val.get("ready_error").and_then(|v| v.as_str());
                    let warning = val.get("warning").and_then(|v| v.as_str());

                    if no_wait || ready.is_none() {
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
                        return;
                    }

                    match ready {
                        Some(true) => {
                            if let Some(pid) = pid {
                                println!("Spawned: {} (pane {}) [ready]", title_str, pid);
                            } else {
                                print_object(&val);
                            }
                            if let Some(warning) = warning {
                                eprintln!("Warning: {}", warning);
                            }
                        }
                        Some(false) => {
                            let reason = ready_error.unwrap_or("unknown reason");
                            eprintln!("Error: spawn failed — {}", reason);
                            if let Some(pid) = pid {
                                eprintln!("       pane: {} (you may want to inspect or close it)", pid);
                            }
                            std::process::exit(1);
                        }
                        None => unreachable!(),
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Compact { pane } => {
            let pane_id = resolve_pane(client, pane);
            let cmd = "/compact\n";
            match http.post(format!("{}/api/panes/{}/send", base, pane_id))
                .json(&serde_json::json!({"text": cmd}))
                .send()
            {
                Ok(_) => { println!("Sent /compact to {}", pane_id); }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Restart { pane, force, grace_secs } => {
            let pane_id = resolve_pane(client, pane);
            let body = serde_json::json!({"force": force, "grace_secs": grace_secs});
            match http.post(format!("{}/api/panes/{}/agent/restart", base, pane_id))
                .json(&body)
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let retry = resp.headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("30");
                        eprintln!("Error: rate limited (retry after {}s)", retry);
                        std::process::exit(1);
                    }
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if !status.is_success() {
                        let msg = val.get("error").and_then(|v| v.as_str())
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
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        AgentAction::Model { name, pane } => {
            let valid = ["sonnet", "haiku", "opus"];
            if !valid.contains(&name.as_str()) {
                eprintln!("Invalid model: {}. Use: sonnet, haiku, opus", name);
                std::process::exit(1);
            }
            let pane_id = resolve_pane(client, pane);
            let cmd = format!("/model {}\n", name);
            match http.post(format!("{}/api/panes/{}/send", base, pane_id))
                .json(&serde_json::json!({"text": cmd}))
                .send()
            {
                Ok(_) => { println!("Sent /model {} to {}", name, pane_id); }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
    }
}
