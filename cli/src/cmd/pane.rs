use crate::client::SocketClient;
use crate::cmd::print::handle_result;
use crate::{build_http, read_api_config, resolve_pane, PaneAction};

pub fn handle_pane(client: &mut SocketClient, json: bool, action: PaneAction) {
    match action {
        PaneAction::List => {
            let result = client.call("pane.list", None);
            handle_result(json, result);
        }
        PaneAction::New { workspace, cwd, title, cmd } => {
            let (base, token) = read_api_config();
            let http = build_http(&token);
            let mut body = serde_json::json!({});
            if let Some(w) = workspace { body["workspace_id"] = serde_json::Value::String(w); }
            if let Some(c) = cwd { body["cwd"] = serde_json::Value::String(c); }
            if let Some(t) = title { body["title"] = serde_json::Value::String(t); }
            let create_result = http.post(format!("{}/api/panes", base))
                .json(&body)
                .send()
                .map_err(|e| e.to_string())
                .and_then(|r| {
                    let status = r.status();
                    let txt = r.text().unwrap_or_default();
                    if status.is_success() {
                        serde_json::from_str::<serde_json::Value>(&txt).map_err(|e| e.to_string())
                    } else {
                        Err(format!("HTTP {}: {}", status.as_u16(), txt))
                    }
                });

            let final_result = match (create_result, cmd) {
                (Ok(resp), Some(cmd_text)) => {
                    let pane_id = resp.get("pane_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if pane_id.is_empty() {
                        Ok(resp)
                    } else {
                        let exec_url = format!("{}/api/panes/{}/exec", base, pane_id);
                        let exec_body = serde_json::json!({"command": cmd_text});
                        match http.post(&exec_url).json(&exec_body).send() {
                            Ok(r) if r.status().is_success() => Ok(resp),
                            Ok(r) => Err(format!("pane created (id={}) but --cmd failed: HTTP {}: {}",
                                pane_id, r.status().as_u16(), r.text().unwrap_or_default())),
                            Err(e) => Err(format!("pane created (id={}) but --cmd failed: {}", pane_id, e)),
                        }
                    }
                }
                (other, _) => other,
            };
            handle_result(json, final_result);
        }
        PaneAction::Current { last } => {
            let result = client.call("pane.list", None);
            match result {
                Ok(value) => {
                    if let Some(arr) = value.as_array() {
                        let pane = if last {
                            arr.last()
                        } else {
                            arr.iter()
                                .find(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                                .or_else(|| arr.first())
                        };
                        if let Some(id) = pane.and_then(|p| p.get("id").and_then(|v| v.as_str())) {
                            println!("{}", id);
                        }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        PaneAction::Focus { id } => {
            let result = client.call("pane.focus", Some(serde_json::json!({ "pane_id": id })));
            handle_result(json, result);
        }
        PaneAction::Close { id } => {
            let result = client.call("pane.close", Some(serde_json::json!({ "pane_id": id })));
            handle_result(json, result);
        }
        PaneAction::Rename { pane, title } => {
            let pane_id = resolve_pane(client, pane);
            let result = client.call(
                "pane.rename",
                Some(serde_json::json!({ "pane_id": pane_id, "title": title })),
            );
            handle_result(json, result);
        }
        PaneAction::GetText { pane, formatted } => {
            let pane_id = resolve_pane(client, pane);
            let (api_url, api_token) = read_api_config();
            let http = build_http(&api_token);
            let url = format!("{}/api/panes/{}/text?formatted={}", api_url, pane_id, formatted);
            match http.get(url).send() {
                Ok(resp) => {
                    if let Ok(val) = resp.json::<serde_json::Value>() {
                        if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
                            if formatted {
                                print!("{}", text);
                            } else {
                                for line in text.lines() {
                                    if !line.trim().is_empty() {
                                        println!("{}", line);
                                    }
                                }
                            }
                            return;
                        }
                    }
                    eprintln!("No pane text");
                    std::process::exit(1);
                }
                Err(e) => { eprintln!("Get-text request failed: {}", e); std::process::exit(1); }
            }
        }
        PaneAction::Exec { pane, command, timeout } => {
            let pane_id = resolve_pane(client, pane);
            let (api_url, api_token) = read_api_config();
            let http = build_http(&api_token);
            match http.post(format!("{}/api/panes/{}/exec", api_url, pane_id))
                .json(&serde_json::json!({
                    "command": command,
                    "timeout_ms": timeout,
                    "strip_ansi": true,
                }))
                .send()
            {
                Ok(resp) => {
                    if let Ok(val) = resp.json::<serde_json::Value>() {
                        if let Some(output) = val.get("output").and_then(|v| v.as_str()) {
                            print!("{}", output);
                            return;
                        }
                    }
                    eprintln!("Exec failed");
                    std::process::exit(1);
                }
                Err(e) => { eprintln!("Exec request failed: {}", e); std::process::exit(1); }
            }
        }
        PaneAction::Find { pattern } => {
            let result = client.call("pane.list", None);
            match result {
                Ok(value) => {
                    let arr = value.as_array().cloned().unwrap_or_default();
                    let needle = pattern.to_lowercase();
                    let needle_compact: String = needle.chars()
                        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
                        .collect();
                    let mut scored: Vec<(i32, &serde_json::Value)> = arr.iter()
                        .filter_map(|p| {
                            let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                            let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                            let sname = p.get("agent_session_name").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                            let agent = p.get("agent_type").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                            let sname_compact: String = sname.chars()
                                .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
                                .collect();
                            let mut score = 0i32;
                            if id.starts_with(&needle) { score += 100; }
                            if sname == needle { score += 80; }
                            if sname.contains(&needle) { score += 50; }
                            if !needle_compact.is_empty() && sname_compact.contains(&needle_compact) {
                                score += 40;
                            }
                            if title == needle { score += 35; }
                            if title.contains(&needle) { score += 20; }
                            if agent.contains(&needle) { score += 10; }
                            if score > 0 { Some((score, p)) } else { None }
                        })
                        .collect();
                    scored.sort_by(|a, b| b.0.cmp(&a.0));
                    if json {
                        let out: Vec<&serde_json::Value> = scored.iter().map(|(_, p)| *p).collect();
                        println!("{}", serde_json::to_string_pretty(&out).unwrap());
                    } else if scored.is_empty() {
                        println!("No panes match: {}", pattern);
                    } else {
                        for (_, p) in &scored {
                            let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("");
                            let agent = p.get("agent_type").and_then(|v| v.as_str()).unwrap_or("-");
                            let sname = p.get("agent_session_name").and_then(|v| v.as_str()).unwrap_or("");
                            let extra = if sname.is_empty() { String::new() } else { format!("  session={}", sname) };
                            println!("{}  {} [{}]{}", id, title, agent, extra);
                        }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        PaneAction::Status { pane } => {
            let pane_id = resolve_pane(client, pane);
            let (api_url, api_token) = read_api_config();
            let http = build_http(&api_token);
            match http.get(format!("{}/api/panes", api_url)).send() {
                Ok(resp) => {
                    if let Ok(panes) = resp.json::<Vec<serde_json::Value>>() {
                        if let Some(p) = panes.iter().find(|p| p.get("id").and_then(|v| v.as_str()) == Some(&pane_id)) {
                            let at = p.get("agent_type").and_then(|v| v.as_str()).unwrap_or("-");
                            let sid = p.get("agent_session_id").and_then(|v| v.as_str()).unwrap_or("-");
                            let pp = if p.get("pending_prompt").is_some() { "yes" } else { "no" };
                            let cv = p.get("chat_view").and_then(|v| v.as_bool()).unwrap_or(false);
                            println!("pane:    {}", pane_id);
                            println!("agent:   {}", at);
                            println!("session: {}", sid);
                            println!("prompt:  {}", pp);
                            println!("chat:    {}", cv);
                            return;
                        }
                    }
                    eprintln!("Pane not found");
                    std::process::exit(1);
                }
                Err(e) => { eprintln!("Status request failed: {}", e); std::process::exit(1); }
            }
        }
    }
}
