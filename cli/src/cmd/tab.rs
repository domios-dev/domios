use crate::client::SocketClient;
use crate::cmd::agent::normalize_tab_description;
use crate::cmd::print::{handle_result, print_json};
use crate::{build_http, read_api_config, resolve_tab, TabAction};

pub fn handle_tab(client: &mut SocketClient, json: bool, action: TabAction) {
    match action {
        TabAction::List {
            workspace,
            all_workspaces,
            agents,
        } => {
            let caller_tab_id = std::env::var("CODETERM_TAB_ID").ok();
            let workspace_id = if all_workspaces {
                None
            } else {
                workspace
                    .or_else(|| std::env::var("CODETERM_WORKSPACE_ID").ok())
                    .or_else(|| caller_workspace_id(client, caller_tab_id.as_deref()))
            };
            let params = tab_list_params(workspace_id, all_workspaces, agents, caller_tab_id);
            match client.call("tab.list", Some(params)) {
                Ok(value) => {
                    let Some(arr) = value.as_array() else {
                        handle_result(json, Ok(value));
                        return;
                    };
                    let mut list = arr.clone();
                    if let Err(error) = append_remote_workers(&mut list) {
                        eprintln!("warning: remote team members unavailable: {error}");
                    }
                    if json {
                        print_json(&serde_json::Value::Array(list));
                    } else if list.is_empty() {
                        println!("(no tabs)");
                    } else {
                        let current_tab = std::env::var("CODETERM_TAB_ID").ok();
                        print!(
                            "{}",
                            render_tab_tree_with_current(&list, current_tab.as_deref())
                        );
                    }
                }
                other => handle_result(json, other),
            }
        }
        TabAction::Inspect { tabs } => {
            let result = client.call("tab.inspect", Some(serde_json::json!({ "tab_ids": tabs })));
            handle_result(json, result);
        }
        TabAction::New {
            workspace,
            cwd,
            title,
            team,
            cmd,
            eager,
            chat_backend,
            config,
        } => {
            let (base, token) = read_api_config();
            let http = match build_http(&token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let mut body = serde_json::json!({});
            if let Some(w) = workspace {
                body["workspace_id"] = serde_json::Value::String(w);
            }
            if let Some(c) = cwd {
                body["cwd"] = serde_json::Value::String(c);
            }
            if let Some(t) = title {
                body["title"] = serde_json::Value::String(t);
            }
            if let Some(team_id) = team {
                let creator_tab_id = std::env::var("CODETERM_TAB_ID")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| {
                        eprintln!("Error: --team requires CODETERM_TAB_ID from a live Team member");
                        std::process::exit(1);
                    });
                body["team_id"] = serde_json::Value::String(team_id);
                body["creator_tab_id"] = serde_json::Value::String(creator_tab_id);
            }
            if eager {
                body["eager"] = serde_json::Value::Bool(true);
            }
            if let Some(plugin_id) = chat_backend {
                body["chat_backend"] = serde_json::Value::String(plugin_id);
                if let Some(cfg) = config.as_deref() {
                    match serde_json::from_str::<serde_json::Value>(cfg) {
                        Ok(v) => body["config"] = v,
                        Err(e) => {
                            eprintln!("Error: --config is not valid JSON: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
            }
            let create_result = http
                .post(format!("{}/api/tabs", base))
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
                    let tab_id = resp
                        .get("tab_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if tab_id.is_empty() {
                        Ok(resp)
                    } else {
                        let exec_url = format!("{}/api/tabs/{}/exec", base, tab_id);
                        let exec_body = serde_json::json!({"command": cmd_text});
                        match http.post(&exec_url).json(&exec_body).send() {
                            Ok(r) if r.status().is_success() => Ok(resp),
                            Ok(r) => Err(format!(
                                "tab created (id={}) but --cmd failed: HTTP {}: {}",
                                tab_id,
                                r.status().as_u16(),
                                r.text().unwrap_or_default()
                            )),
                            Err(e) => Err(format!(
                                "tab created (id={}) but --cmd failed: {}",
                                tab_id, e
                            )),
                        }
                    }
                }
                (other, _) => other,
            };
            handle_result(json, final_result);
        }
        TabAction::Current { last } => {
            // The tab this command RUNS IN is the env var — the focused tab
            // is whatever tab the human is looking at (an orchestrator once
            // adopted a foreign focused tab as its own id and mis-parented
            // eleven workers). Fall back to focus only outside tab context,
            // loudly.
            if !last {
                if let Ok(own) = std::env::var("CODETERM_TAB_ID") {
                    if !own.trim().is_empty() {
                        println!("{}", own.trim());
                        return;
                    }
                }
            }
            let result = client.call("tab.list", None);
            match result {
                Ok(value) => {
                    if let Some(arr) = value.as_array() {
                        let tab = if last {
                            arr.last()
                        } else {
                            eprintln!(
                                "warning: no CODETERM_TAB_ID in this shell — falling back to the FOCUSED tab, which may not be yours"
                            );
                            arr.iter()
                                .find(|p| {
                                    p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false)
                                })
                                .or_else(|| arr.first())
                        };
                        if let Some(id) = tab.and_then(|p| p.get("id").and_then(|v| v.as_str())) {
                            println!("{}", id);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        TabAction::Focus { id } => {
            let result = client.call("tab.focus", Some(serde_json::json!({ "tab_id": id })));
            handle_result(json, result);
        }
        TabAction::Close { id } => {
            let mut params = serde_json::json!({ "tab_id": id });
            crate::cmd::plan::maybe_inject_caller_tab(&mut params);
            let result = client.call("tab.close", Some(params));
            handle_result(json, result);
        }
        TabAction::Rename { tab, title } => {
            let tab_id = resolve_tab(client, tab);
            let result = client.call(
                "tab.rename",
                Some(serde_json::json!({ "tab_id": tab_id, "title": title })),
            );
            handle_result(json, result);
        }
        TabAction::Describe { tab, summary } => {
            let description = match normalize_tab_description(&summary) {
                Ok(Some(d)) => d,
                Ok(None) => String::new(),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            let tab_id = resolve_tab(client, tab);
            let result = client.call(
                "tab.describe",
                Some(serde_json::json!({ "tab_id": tab_id, "description": description })),
            );
            handle_result(json, result);
        }
        TabAction::GetText { tab, formatted } => {
            let tab_id = resolve_tab(client, tab);
            let (api_url, api_token) = read_api_config();
            let http = match build_http(&api_token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let url = format!(
                "{}/api/tabs/{}/text?formatted={}",
                api_url, tab_id, formatted
            );
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
                    eprintln!("No tab text");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Get-text request failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        TabAction::Exec {
            tab,
            command,
            timeout,
        } => {
            let tab_id = resolve_tab(client, tab);
            let (api_url, api_token) = read_api_config();
            let http = match build_http(&api_token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            match http
                .post(format!("{}/api/tabs/{}/exec", api_url, tab_id))
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
                Err(e) => {
                    eprintln!("Exec request failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        TabAction::Search { pattern } => {
            let result = client.call("tab.list", None);
            match result {
                Ok(value) => {
                    let arr = value.as_array().cloned().unwrap_or_default();
                    print_tab_find_results(json, &pattern, &arr);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        TabAction::Status { tab } => {
            let tab_selector = tab.unwrap_or_else(|| resolve_tab(client, None));
            let (api_url, api_token) = read_api_config();
            let http = match build_http(&api_token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            match http
                .get(format!("{}/api/tabs/{}/status", api_url, tab_selector))
                .send()
            {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        eprintln!("Tab not found");
                        std::process::exit(1);
                    }
                    if let Ok(p) = resp.json::<serde_json::Value>() {
                        let tab_id = p
                            .get("tab_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&tab_selector);
                        let at = p.get("agent_type").and_then(|v| v.as_str()).unwrap_or("-");
                        let status = p.get("status").and_then(|v| v.as_str()).unwrap_or("-");
                        let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("-");
                        println!("tab:     {}", tab_id);
                        println!("agent:   {}", at);
                        println!("title:   {}", title);
                        println!("status:  {}", status);
                        return;
                    }
                    eprintln!("Tab not found");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Status request failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        TabAction::Stop { tab } => {
            let tab_id = resolve_tab(client, tab);
            let (api_url, api_token) = read_api_config();
            let http = match build_http(&api_token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            match http
                .post(format!("{}/api/tabs/{}/stop", api_url, tab_id))
                .header("Content-Type", "application/json")
                .body("{}")
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    let body: serde_json::Value =
                        resp.json().unwrap_or_else(|_| serde_json::json!({}));
                    if json {
                        print_json(&body);
                    } else if !status.is_success() {
                        eprintln!("Stop failed: HTTP {}", status);
                        std::process::exit(1);
                    } else if body
                        .get("stopped")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        println!("Stopped tab {}", tab_id);
                    } else {
                        println!("Tab {} has no stop keystroke declared", tab_id);
                    }
                }
                Err(e) => {
                    eprintln!("Stop request failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        TabAction::Chat { tab, since } => {
            let tab_id = resolve_tab(client, tab);
            let (api_url, api_token) = read_api_config();
            let http = match build_http(&api_token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let url = format!("{}/api/tabs/{}/chat?since={}", api_url, tab_id, since);
            match http.get(url).send() {
                // Non-JSON / empty body (e.g. tab not bound yet) -> {} so the renderer prints an honest "(no chat ...)" instead of a decode error.
                Ok(resp) => {
                    let v = resp
                        .json::<serde_json::Value>()
                        .unwrap_or_else(|_| serde_json::json!({}));
                    if json {
                        handle_result(json, Ok(v));
                    } else {
                        print_tab_chat(&v);
                    }
                }
                Err(e) => {
                    eprintln!("Error: chat read failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        TabAction::Capture { action } => handle_tab_capture(client, json, action),
    }
}

fn tab_list_params(
    workspace_id: Option<String>,
    all_workspaces: bool,
    agents_only: bool,
    caller_tab_id: Option<String>,
) -> serde_json::Value {
    let focused_workspace = !all_workspaces && workspace_id.is_none();
    let caller_tab_id = if focused_workspace {
        caller_tab_id
    } else {
        None
    };
    serde_json::json!({
        "workspace_id": workspace_id,
        "caller_tab_id": caller_tab_id,
        "focused_workspace": focused_workspace,
        "agents_only": agents_only,
    })
}

fn caller_workspace_id(client: &mut SocketClient, caller_tab_id: Option<&str>) -> Option<String> {
    let tab_id = caller_tab_id?.trim();
    if tab_id.is_empty() {
        return None;
    }
    client
        .call(
            "tab.inspect",
            Some(serde_json::json!({ "tab_ids": [tab_id] })),
        )
        .ok()?
        .as_array()?
        .first()?
        .get("workspace_id")?
        .as_str()
        .map(str::to_string)
}

fn append_remote_workers(tabs: &mut Vec<serde_json::Value>) -> Result<(), String> {
    let (base, token) = read_api_config();
    let http = build_http(&token).map_err(|e| e.to_string())?;
    let response = http
        .get(format!("{base}/api/remote-workers"))
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), body));
    }
    let registry = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    merge_remote_workers(tabs, &registry);
    Ok(())
}

fn merge_remote_workers(tabs: &mut Vec<serde_json::Value>, registry: &serde_json::Value) {
    let Some(by_orchestrator) = registry.as_object() else {
        return;
    };
    let parents: std::collections::HashMap<String, (String, String)> = tabs
        .iter()
        .filter_map(|tab| {
            Some((
                sfield(tab, "id").to_string(),
                (
                    sfield(tab, "workspace_name").to_string(),
                    sfield(tab, "workspace_id").to_string(),
                ),
            ))
        })
        .collect();
    let mut remote = Vec::new();
    for (orchestrator, workers) in by_orchestrator {
        let Some((workspace_name, workspace_id)) = parents.get(orchestrator) else {
            continue;
        };
        let Some(workers) = workers.as_array() else {
            continue;
        };
        for worker in workers {
            let remote_tab = sfield(worker, "remote_tab");
            if remote_tab.is_empty() {
                continue;
            }
            remote.push(serde_json::json!({
                "id": remote_tab,
                "tab_id": remote_tab,
                "title": sfield(worker, "title"),
                "parent_tab_id": orchestrator,
                "workspace_name": workspace_name,
                "workspace_id": workspace_id,
                "agent_session_model": sfield(worker, "model"),
                "role": "Worker",
                "remote_peer": sfield(worker, "peer"),
                "remote": true,
            }));
        }
    }
    tabs.extend(remote);
}

fn handle_tab_capture(
    client: &mut SocketClient,
    json: bool,
    action: crate::args::misc::TabCaptureAction,
) {
    use crate::args::misc::TabCaptureAction;
    let (api_url, api_token) = read_api_config();
    let http = match build_http(&api_token) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to build HTTP client: {}", e);
            std::process::exit(1);
        }
    };
    let (tab, url, body) = match action {
        TabCaptureAction::Start { tab, out } => {
            let tab_id = resolve_tab(client, tab);
            let url = format!("{}/api/tabs/{}/capture/start", api_url, tab_id);
            (tab_id, url, serde_json::json!({ "out": out }))
        }
        TabCaptureAction::Stop { tab } => {
            let tab_id = resolve_tab(client, tab);
            let url = format!("{}/api/tabs/{}/capture/stop", api_url, tab_id);
            (tab_id, url, serde_json::json!({}))
        }
    };
    let _ = tab;
    match http.post(url).json(&body).send() {
        Ok(resp) => {
            let v = resp
                .json::<serde_json::Value>()
                .unwrap_or_else(|_| serde_json::json!({}));
            handle_result(json, Ok(v));
        }
        Err(e) => {
            eprintln!("Error: capture request failed: {}", e);
            std::process::exit(1);
        }
    }
}

/// `-=-codeterm:tab label=X tab=Y ...-=-body` → (Some("X"), "body"). Plain text → (None, text).
fn strip_origin_marker(s: &str) -> (Option<String>, String) {
    if let Some(rest) = s.strip_prefix("-=-codeterm:") {
        if let Some(end) = rest.find("-=-") {
            let label = rest[..end]
                .split_whitespace()
                .find_map(|kv| kv.strip_prefix("label="))
                .map(|l| l.replace("%20", " "));
            return (label, rest[end + 3..].to_string());
        }
    }
    (None, s.to_string())
}

/// One-line, length-capped view of a message body so a single huge blob
/// (skill context, AGENTS.md injection, big tool output) can't swamp the
/// transcript and hide the real turns.
fn truncate_for_chat(s: &str, max: usize) -> String {
    let oneline = s.trim().replace('\n', " ⏎ ");
    let n = oneline.chars().count();
    if n <= max {
        return oneline;
    }
    let cut: String = oneline.chars().take(max).collect();
    format!("{}… [+{} chars]", cut, n - max)
}

/// Human-readable transcript: one capped line per message, tool calls show
/// the command, tool results show their output, origin markers become `(from X)`.
fn print_tab_chat(v: &serde_json::Value) {
    let Some(msgs) = v.get("messages").and_then(|m| m.as_array()) else {
        println!("(no chat transcript)");
        return;
    };
    if msgs.is_empty() {
        println!("(no chat messages yet)");
        return;
    }
    for m in msgs {
        let role = m.get("type").and_then(|t| t.as_str()).unwrap_or("msg");
        let tool_args = m.get("toolArgs").and_then(|t| t.as_str());
        let tool_result = m.get("toolResult").and_then(|t| t.as_str());
        let content = m.get("content").and_then(|t| t.as_str()).unwrap_or("");

        let (label, origin, raw) = if let Some(ta) = tool_args {
            ("⚙ tool", None, ta.to_string())
        } else if let Some(tr) = tool_result {
            ("  ↳ result", None, tr.to_string())
        } else {
            let (origin, body) = strip_origin_marker(content);
            let label = match role {
                "assistant" => "● assistant",
                "thinking" => "  ∴ thinking",
                "system" => "  · system",
                _ => "▸ user",
            };
            (label, origin, body)
        };

        let body = truncate_for_chat(&raw, 280);
        if body.is_empty() {
            continue;
        }
        match origin {
            Some(from) => println!("{} (from {}) {}", label, from, body),
            None => println!("{} {}", label, body),
        }
    }
}

/// Rank and print tab-find results. Shared between the local socket path
/// (handle_tab) and the mesh-relayed path (main.rs intercepts `--mesh
/// Tab find` so the ranker runs over the peer's list).
pub fn print_tab_find_results(json: bool, pattern: &str, tabs: &[serde_json::Value]) {
    let scored = rank_tabs(pattern, tabs);
    if json {
        let out = serde_json::Value::Array(scored.into_iter().map(|(_, p)| p.clone()).collect());
        print_json(&out);
    } else if scored.is_empty() {
        println!("No tabs match: {}", pattern);
    } else {
        for (_, p) in &scored {
            println!("{}", format_tab_row(p));
        }
    }
}

/// Fuzzy ranker over id / tab.title / session name / tab.title / agent / model. Cross-script via deunicode.
pub fn rank_tabs<'a>(
    pattern: &str,
    tabs: &'a [serde_json::Value],
) -> Vec<(i32, &'a serde_json::Value)> {
    let needle = pattern.to_lowercase();
    let needle_translit = translit(&needle);
    let needle_compact = strip_separators(&needle);
    let needle_translit_compact = strip_separators(&needle_translit);
    let mut scored: Vec<(i32, &serde_json::Value)> = tabs
        .iter()
        .filter_map(|p| {
            let id = field(p, "id");
            let title = field(p, "title");
            let tab_title = field(p, "tab_title");
            let ws_name = field(p, "workspace_name");
            let sname = field(p, "agent_session_name");
            let agent = field(p, "agent_type");
            let model = field(p, "agent_session_model");
            let mut score = 0i32;
            // id prefix only counts at ≥4 chars — one-letter needles like "a" otherwise match ~6% of all UUIDs and drown the real result.
            if needle.len() >= 4 && id.starts_with(&needle) {
                score += 100;
            }
            score += score_field(&sname, &needle, &needle_translit, 80, 50);
            score += score_field(&tab_title, &needle, &needle_translit, 75, 45);
            if !needle_compact.is_empty() || !needle_translit_compact.is_empty() {
                let sname_c = strip_separators(&sname);
                let sname_c_translit = strip_separators(&translit(&sname));
                if !sname_c.is_empty()
                    && ((!needle_compact.is_empty() && sname_c.contains(&needle_compact))
                        || (!needle_translit_compact.is_empty()
                            && sname_c_translit.contains(&needle_translit_compact)))
                {
                    score += 40;
                }
                let tab_c = strip_separators(&tab_title);
                let tab_c_translit = strip_separators(&translit(&tab_title));
                if !tab_c.is_empty()
                    && ((!needle_compact.is_empty() && tab_c.contains(&needle_compact))
                        || (!needle_translit_compact.is_empty()
                            && tab_c_translit.contains(&needle_translit_compact)))
                {
                    score += 38;
                }
            }
            score += score_field(&title, &needle, &needle_translit, 35, 20);
            if !ws_name.is_empty() {
                let ws_translit = translit(&ws_name);
                if ws_name.contains(&needle) || ws_translit.contains(&needle_translit) {
                    score += 15;
                }
            }
            if !agent.is_empty() && agent.contains(&needle) {
                score += 10;
            }
            if !model.is_empty() && model.contains(&needle) {
                score += 10;
            }
            if score > 0 {
                Some((score, p))
            } else {
                None
            }
        })
        .collect();
    // Stable tie-break by id so identical scores keep a deterministic order.
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            let ai = a.1.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let bi = b.1.get("id").and_then(|v| v.as_str()).unwrap_or("");
            ai.cmp(bi)
        })
    });
    scored
}

fn field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Like `field` but preserves case — for display, not fuzzy matching.
fn sfield<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn strip_separators(s: &str) -> String {
    // "Dalton" tab. Same for whitespace / dash / underscore which commonly differ between user input and stored titles.
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_' && *c != '\'')
        .collect()
}

/// ASCII-lowercase transliteration. `deunicode` already handles Cyrillic →
/// Latin (Далтон → Dalton), Greek, Jatabse, etc. Returns the lowered form
/// so the result is symmetric with `needle.to_lowercase()`.
fn translit(s: &str) -> String {
    deunicode::deunicode(s).to_lowercase()
}

/// Score one haystack field with both exact-match (high) and contains
/// (medium) tiers, falling back to the transliterated form if the raw
/// match misses. Returning 0 means no contribution.
fn score_field(
    haystack: &str,
    needle: &str,
    needle_translit: &str,
    exact: i32,
    contains: i32,
) -> i32 {
    if haystack.is_empty() {
        return 0;
    }
    if haystack == needle {
        return exact;
    }
    let h_translit = translit(haystack);
    if h_translit == needle_translit && !needle_translit.is_empty() {
        return exact;
    }
    if haystack.contains(needle) {
        return contains;
    }
    if !needle_translit.is_empty() && h_translit.contains(needle_translit) {
        return contains;
    }
    0
}

/// One-line human row used by both `Tab list` (when we switch off the
/// default kv printer) and `Tab find`. Shows id  tab/title  [provider:model]
/// so the user gets the same info they see in the sidebar.
pub fn format_tab_row(p: &serde_json::Value) -> String {
    let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    // One label — the sidebar name. Tab title wins (that's what the user sees and what `tab rename` updates); falls back to tab.title only when the tab title is missing.
    let label = {
        let tab_title = p.get("tab_title").and_then(|v| v.as_str()).unwrap_or("");
        let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("");
        if !tab_title.is_empty() {
            tab_title
        } else {
            title
        }
    };
    let agent = p.get("agent_type").and_then(|v| v.as_str()).unwrap_or("-");
    let model = p
        .get("agent_session_model")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let model_part = if model.is_empty() {
        String::new()
    } else {
        format!("/{}", model)
    };
    let description = p.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let description_part = format_description_suffix(description);
    let ws = p
        .get("workspace_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let ws_part = if ws.is_empty() {
        String::new()
    } else {
        format!("  ws={}", ws)
    };
    format!(
        "{}  {}{} [{}{}]{}",
        id, label, description_part, agent, model_part, ws_part
    )
}

/// Human `Tab list`: workspaces as headers, tabs as `<id> <name> <model>`
/// rows, sub-agents nested under their parent by `parent_tab_id`. Compact
/// on purpose — this output is read by agents too, so every line costs tokens.
pub fn render_tab_tree(tabs: &[serde_json::Value]) -> String {
    render_tab_tree_with_current(tabs, None)
}

pub fn render_tab_tree_with_current(
    tabs: &[serde_json::Value],
    current_tab_id: Option<&str>,
) -> String {
    struct Row {
        id: String,
        label: String,
        model: String,
        description: String,
        parent: Option<String>,
        role: String,
        peer: String,
        team_id: String,
        creator_tab_id: String,
        is_current: bool,
    }
    // HashMap-unordered).
    let mut groups: std::collections::BTreeMap<(String, String), Vec<Row>> =
        std::collections::BTreeMap::new();
    for p in tabs {
        let id = sfield(p, "id").to_string();
        if id.is_empty() {
            continue;
        }
        let tab_title = sfield(p, "tab_title");
        let label = if !tab_title.is_empty() {
            tab_title
        } else {
            sfield(p, "title")
        }
        .to_string();
        let model_raw = sfield(p, "agent_session_model");
        let model = if !model_raw.is_empty() {
            model_raw
        } else {
            sfield(p, "agent_type")
        }
        .to_string();
        let description = sfield(p, "description").to_string();
        let role = sfield(p, "role").to_ascii_lowercase();
        let peer = sfield(p, "remote_peer").to_string();
        let team_id = sfield(p, "team_id").to_string();
        let creator_tab_id = sfield(p, "spawned_by_tab_id").to_string();
        let parent = p
            .get("parent_tab_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ws_name = sfield(p, "workspace_name").to_string();
        let ws_id = sfield(p, "workspace_id").to_string();
        groups.entry((ws_name, ws_id)).or_default().push(Row {
            is_current: current_tab_id == Some(id.as_str()),
            id,
            label,
            model,
            description,
            parent,
            role,
            peer,
            team_id,
            creator_tab_id,
        });
    }

    let mut out = String::new();
    for (gi, ((ws_name, ws_id), rows)) in groups.iter().enumerate() {
        if gi > 0 {
            out.push('\n');
        }
        let name = if ws_name.is_empty() {
            "(detached)"
        } else {
            ws_name.as_str()
        };
        if ws_id.is_empty() {
            out.push_str(&format!("{}\n", name));
        } else {
            out.push_str(&format!("{}  {}\n", name, ws_id));
        }

        // Nest by parent within the workspace. A tab whose parent lives in another workspace (or has none) is a root here.
        let ids: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        let mut children: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        let mut roots: Vec<usize> = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            match &r.parent {
                Some(par) if ids.contains(par.as_str()) => {
                    children.entry(par.as_str()).or_default().push(i);
                }
                _ => roots.push(i),
            }
        }
        let label_w = rows
            .iter()
            .map(|r| r.label.chars().count())
            .max()
            .unwrap_or(0)
            .min(24);
        let labels_by_id: std::collections::HashMap<&str, &str> = rows
            .iter()
            .map(|row| (row.id.as_str(), row.label.as_str()))
            .collect();
        let row_rank = |row: &Row| {
            if !row.team_id.is_empty() && row.role.is_empty() {
                2
            } else if row.role == "worker" {
                1
            } else {
                0
            }
        };
        let by_label = |a: &usize, b: &usize| {
            row_rank(&rows[*a])
                .cmp(&row_rank(&rows[*b]))
                .then_with(|| rows[*a].label.cmp(&rows[*b].label))
        };
        roots.sort_by(by_label);

        // Iterative DFS so deep nesting can't blow the stack.
        let mut stack: Vec<(usize, usize)> = roots.iter().rev().map(|&i| (i, 0usize)).collect();
        while let Some((i, depth)) = stack.pop() {
            let r = &rows[i];
            let indent = "  ".repeat(depth + 1);
            let current_suffix = if r.is_current { " (self)" } else { "" };
            let utility_suffix = if !r.team_id.is_empty() && r.role.is_empty() {
                let creator = labels_by_id
                    .get(r.creator_tab_id.as_str())
                    .copied()
                    .filter(|label| !label.is_empty())
                    .map(|label| format!("terminal by {label}"))
                    .unwrap_or_else(|| {
                        let id = if r.creator_tab_id.is_empty() {
                            "unknown"
                        } else {
                            r.creator_tab_id.as_str()
                        };
                        format!("terminal by archived {id}")
                    });
                Some(creator)
            } else {
                None
            };
            let role_suffix = match (
                utility_suffix.as_deref().unwrap_or(r.role.as_str()),
                r.peer.as_str(),
            ) {
                ("", "") => String::new(),
                (role, "") => format!(" [{}]", role),
                ("", peer) => format!(" [@{}]", peer),
                (role, peer) => format!(" [{} @{}]", role, peer),
            };
            if r.model.is_empty() {
                out.push_str(&format!(
                    "{}{} {}{}{}{}\n",
                    indent,
                    r.id,
                    r.label,
                    format_description_suffix(&r.description),
                    role_suffix,
                    current_suffix
                ));
            } else {
                out.push_str(&format!(
                    "{}{} {:<w$} {}{}{}{}\n",
                    indent,
                    r.id,
                    r.label,
                    r.model,
                    format_description_suffix(&r.description),
                    role_suffix,
                    current_suffix,
                    w = label_w
                ));
            }
            if let Some(kids) = children.get(r.id.as_str()) {
                let mut kids = kids.clone();
                kids.sort_by(by_label);
                for &k in kids.iter().rev() {
                    stack.push((k, depth + 1));
                }
            }
        }
    }
    out
}

fn format_description_suffix(description: &str) -> String {
    if description.is_empty() {
        String::new()
    } else {
        format!(" - {}", description)
    }
}

#[cfg(test)]
mod tests;
