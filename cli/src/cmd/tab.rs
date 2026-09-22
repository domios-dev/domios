use super::tab_restart::handle_shell_restart;
use crate::client::{RpcClient, SocketClient};
use crate::cmd::agent::normalize_tab_description;
use crate::cmd::print::{
    exit_with_cli_error, handle_cli_result, handle_result, print_json,
    tab_list_json_envelope_with_request, CliError,
};
use crate::{build_local_api, read_api_config, resolve_tab, TabAction};

pub(crate) const INVALID_REMOTE_TAB_ID: &str = "invalid remote tab id";

mod adopt;
mod capture;
mod chat_view;
mod inspect;
pub(crate) mod labels;
pub(crate) mod remote_target;
mod render;
mod repair;
mod search;
mod team_utility;
mod workspace_topology;
use adopt::handle_tab_adopt_agent;
use capture::handle_tab_capture;
use chat_view::print_tab_chat;
use inspect::handle_tab_inspect;
#[cfg(test)]
use remote_target::remote_inspect_target;
use remote_target::{
    classify_remote_tab_id, parse_remote_tab_id, remote_tab_chat_path, RemoteTabTarget,
};
#[cfg(test)]
use render::format_tab_row;
pub use render::render_tab_tree_with_scope;
#[cfg(test)]
use render::{render_tab_tree, render_tab_tree_with_current};
use repair::handle_tab_repair;
pub use search::print_tab_find_results;
#[cfg(test)]
use search::rank_tabs;

pub fn handle_tab(client: &mut SocketClient, json: bool, action: TabAction) {
    if let Err(error) = validate_user_tab_selectors(&action) {
        exit_with_cli_error(json, &CliError::new(error));
    }
    match action {
        TabAction::List {
            workspace,
            all_workspaces,
            agents,
        } => {
            let requested_workspace = workspace.clone();
            if !json && requested_workspace.is_none() {
                if let Err(error) = workspace_topology::print_local_workspace_map(client, agents) {
                    eprintln!("Error: {error}");
                    std::process::exit(1);
                }
                return;
            }
            let workspace_id = workspace;
            let params = tab_list_params(workspace_id, all_workspaces, agents);
            match fetch_ordinary_tabs(&params) {
                Ok(value) => {
                    let Some(arr) = value.as_array() else {
                        handle_result(json, Ok(value));
                        return;
                    };
                    let list = arr.clone();
                    if json {
                        print_json(&tab_list_json_envelope_with_request(
                            list.clone(),
                            requested_workspace.as_deref(),
                        ));
                    } else {
                        let current_tab = std::env::var("CODETERM_TAB_ID").ok();
                        print!(
                            "{}",
                            render_tab_tree_with_scope(
                                &list,
                                current_tab.as_deref(),
                                requested_workspace.as_deref(),
                            )
                        );
                    }
                }
                other => handle_result(json, other),
            }
        }
        TabAction::Inspect {
            tabs,
            tab,
            lifecycle,
        } => {
            handle_tab_inspect(client, json, tabs, tab, lifecycle);
        }
        TabAction::New {
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
            let (base, _) = read_api_config();
            let api = match build_local_api() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to connect to local API: {}", e);
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
            // Caller identity lets the daemon inherit the calling tab's own
            // workspace instead of falling back to the focused one.
            let caller_tab_id = std::env::var("CODETERM_TAB_ID")
                .ok()
                .filter(|value| !value.trim().is_empty());
            if let Some(caller) = caller_tab_id.as_deref() {
                body["creator_tab_id"] = serde_json::Value::String(caller.to_string());
            }
            if let Some(team_id) = team.as_deref() {
                if caller_tab_id.is_none() {
                    eprintln!("Error: --team requires CODETERM_TAB_ID from a live Team member");
                    std::process::exit(1);
                }
                body["team_id"] = serde_json::Value::String(team_id.to_string());
            }
            if no_team {
                body["detach_from_team"] = serde_json::Value::Bool(true);
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
            let create_result = api
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
                        match api.post(&exec_url).json(&exec_body).send() {
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
            team_utility::handle_create_result(json, team.as_deref(), final_result);
        }
        TabAction::Current { last } => {
            // The env var is the tab this RUNS IN; focus is whatever the human
            // looks at. Adopting focus inside a tab mis-parents the caller.
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
        TabAction::Focus { id, tab } => {
            let id = crate::args::misc::required_tab_selector(&id, &tab, "focus").unwrap_or_else(
                |error| {
                    eprintln!("Error: {error}");
                    std::process::exit(2);
                },
            );
            if let RemoteTabTarget::Remote(peer, remote_tab) = classify_remote_tab_id(id) {
                let path = format!("/api/tabs/{}/focus", urlencoding::encode(&remote_tab));
                handle_result(json, Ok(relay_tab_json(&peer, "POST", &path, None)));
                return;
            }
            let result = focus_tab(client, id);
            handle_cli_result(json, result);
        }
        TabAction::Close { id, tab } => {
            let id = crate::args::misc::required_tab_selector(&id, &tab, "close").unwrap_or_else(
                |error| {
                    eprintln!("Error: {error}");
                    std::process::exit(2);
                },
            );
            if let RemoteTabTarget::Remote(peer, remote_tab) = classify_remote_tab_id(id) {
                let path = format!("/api/tabs/{}/close", urlencoding::encode(&remote_tab));
                handle_result(json, Ok(relay_tab_json(&peer, "POST", &path, None)));
                return;
            }
            let mut params = serde_json::json!({ "tab_id": id });
            crate::cmd::plan::maybe_inject_caller_tab(&mut params);
            let result = close_tab(client, params);
            handle_cli_result(json, result);
        }
        TabAction::Restart { tab, cwd, cmd } => {
            handle_shell_restart(client, json, tab, cwd, cmd);
        }
        TabAction::Rename { tab, title } => {
            if let Some((peer, remote_tab)) = tab.as_deref().and_then(parse_remote_tab_id) {
                let body = serde_json::json!({ "title": title }).to_string();
                let data = relay_tab_json(
                    &peer,
                    "POST",
                    &format!("/api/tabs/{}/rename", urlencoding::encode(&remote_tab)),
                    Some(&body),
                );
                handle_result(json, Ok(data));
                return;
            }
            if tab
                .as_deref()
                .is_some_and(|id| matches!(classify_remote_tab_id(id), RemoteTabTarget::Invalid))
            {
                exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
            }
            let tab_id = resolve_tab(client, tab);
            let result = rename_tab(client, &tab_id, &title);
            handle_cli_result(json, result);
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
            match tab.as_deref().map(classify_remote_tab_id) {
                Some(RemoteTabTarget::Remote(peer, remote_tab)) => {
                    let body = serde_json::json!({ "description": description }).to_string();
                    let path = format!("/api/tabs/{}/describe", urlencoding::encode(&remote_tab));
                    handle_result(json, Ok(relay_tab_json(&peer, "POST", &path, Some(&body))));
                    return;
                }
                Some(RemoteTabTarget::Invalid) => {
                    exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
                }
                _ => {}
            }
            let tab_id = resolve_tab(client, tab);
            let result = describe_tab(client, &tab_id, &description);
            handle_cli_result(json, result);
        }
        TabAction::GetText { tab, formatted } => {
            if let Some((peer, remote_tab)) = tab.as_deref().and_then(parse_remote_tab_id) {
                let data = relay_tab_json(
                    &peer,
                    "GET",
                    &format!(
                        "/api/tabs/{}/text?formatted={formatted}",
                        urlencoding::encode(&remote_tab)
                    ),
                    None,
                );
                if let Some(text) = data.get("text").and_then(|value| value.as_str()) {
                    print!("{text}");
                    return;
                }
                eprintln!("No tab text");
                std::process::exit(1);
            }
            if tab
                .as_deref()
                .is_some_and(|id| matches!(classify_remote_tab_id(id), RemoteTabTarget::Invalid))
            {
                exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
            }
            let tab_id = resolve_tab(client, tab);
            let (api_url, _) = read_api_config();
            let http = match build_local_api() {
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
            if let Some((peer, remote_tab)) = tab.as_deref().and_then(parse_remote_tab_id) {
                let body = serde_json::json!({"command": command, "timeout_ms": timeout, "strip_ansi": true}).to_string();
                let data = relay_tab_json(
                    &peer,
                    "POST",
                    &format!("/api/tabs/{}/exec", urlencoding::encode(&remote_tab)),
                    Some(&body),
                );
                if let Some(output) = data.get("output").and_then(|value| value.as_str()) {
                    print!("{output}");
                    return;
                }
                eprintln!("Exec failed");
                std::process::exit(1);
            }
            if tab
                .as_deref()
                .is_some_and(|id| matches!(classify_remote_tab_id(id), RemoteTabTarget::Invalid))
            {
                exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
            }
            let tab_id = resolve_tab(client, tab);
            let (api_url, _) = read_api_config();
            let api = match build_local_api() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to connect to local API: {}", e);
                    std::process::exit(1);
                }
            };
            match api
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
            let result = fetch_search_tabs(client);
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
            match tab.as_deref().map(classify_remote_tab_id) {
                Some(RemoteTabTarget::Remote(peer, remote_tab)) => {
                    let path = format!("/api/tabs/{}/status", urlencoding::encode(&remote_tab));
                    handle_result(json, Ok(relay_tab_json(&peer, "GET", &path, None)));
                    return;
                }
                Some(RemoteTabTarget::Invalid) => {
                    exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
                }
                _ => {}
            }
            let tab_selector = tab.unwrap_or_else(|| resolve_tab(client, None));
            let (api_url, _) = read_api_config();
            let http = match build_local_api() {
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
                        if json {
                            print_json(&p);
                        } else {
                            println!("tab:     {}", tab_id);
                            println!("agent:   {}", at);
                            println!("title:   {}", title);
                            println!("status:  {}", status);
                            for line in status_prompt_lines(&p) {
                                println!("{line}");
                            }
                        }
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
        TabAction::Respond { option, tab } => {
            if option == 0 {
                exit_with_cli_error(
                    json,
                    &CliError::new("tab respond option must be at least 1"),
                );
            }
            match tab.as_deref().map(classify_remote_tab_id) {
                Some(RemoteTabTarget::Remote(peer, remote_tab)) => {
                    let path = format!(
                        "/api/tabs/{}/prompt/respond",
                        urlencoding::encode(&remote_tab)
                    );
                    let body = serde_json::json!({"value": option.to_string()}).to_string();
                    handle_result(json, Ok(relay_tab_json(&peer, "POST", &path, Some(&body))));
                    return;
                }
                Some(RemoteTabTarget::Invalid) => {
                    exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
                }
                _ => {}
            }
            let tab_id = resolve_tab(client, tab);
            let (api_url, _) = read_api_config();
            let api = match build_local_api() {
                Ok(client) => client,
                Err(error) => {
                    exit_with_cli_error(
                        json,
                        &CliError::new(format!("Prompt response failed: {error}")),
                    );
                }
            };
            let result = api
                .post(format!("{api_url}/api/tabs/{tab_id}/prompt/respond"))
                .json(&serde_json::json!({"value": option.to_string()}))
                .send()
                .map_err(|error| error.to_string())
                .and_then(|response| {
                    let status = response.status();
                    let body = response.text().unwrap_or_default();
                    if status.is_success() {
                        serde_json::from_str(&body).map_err(|error| error.to_string())
                    } else {
                        Err(format!("HTTP {}: {body}", status.as_u16()))
                    }
                });
            handle_result(json, result);
        }
        TabAction::AutoConfirm { mode, tab } => {
            let (method, body) = mode.request();
            match tab.as_deref().map(classify_remote_tab_id) {
                Some(RemoteTabTarget::Remote(peer, remote_tab)) => {
                    let path = format!(
                        "/api/tabs/{}/permission-auto-confirm",
                        urlencoding::encode(&remote_tab)
                    );
                    let body = body.map(|value| value.to_string());
                    handle_result(
                        json,
                        Ok(relay_tab_json(&peer, method, &path, body.as_deref())),
                    );
                    return;
                }
                Some(RemoteTabTarget::Invalid) => {
                    exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
                }
                _ => {}
            }
            let tab_id = resolve_tab(client, tab);
            let (api_url, _) = read_api_config();
            let api = match build_local_api() {
                Ok(client) => client,
                Err(error) => {
                    exit_with_cli_error(
                        json,
                        &CliError::new(format!("Auto-confirm request failed: {error}")),
                    );
                }
            };
            let url = format!("{api_url}/api/tabs/{tab_id}/permission-auto-confirm");
            let request = match body {
                Some(body) => api.post(url).json(&body),
                None => api.get(url),
            };
            let result = request
                .send()
                .map_err(|error| error.to_string())
                .and_then(|response| {
                    let status = response.status();
                    let body = response.text().unwrap_or_default();
                    if status.is_success() {
                        serde_json::from_str(&body).map_err(|error| error.to_string())
                    } else {
                        Err(format!("HTTP {}: {body}", status.as_u16()))
                    }
                });
            handle_result(json, result);
        }
        TabAction::Rebind { session_id, tab } => {
            match tab.as_deref().map(classify_remote_tab_id) {
                Some(RemoteTabTarget::Remote(peer, remote_tab)) => {
                    let body = serde_json::json!({"session_id": session_id}).to_string();
                    let path = format!(
                        "/api/tabs/{}/session/rebind",
                        urlencoding::encode(&remote_tab)
                    );
                    print_rebind_result(json, relay_tab_json(&peer, "POST", &path, Some(&body)));
                    return;
                }
                Some(RemoteTabTarget::Invalid) => {
                    exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
                }
                _ => {}
            }
            let tab_id = resolve_tab(client, tab);
            let (api_url, _) = read_api_config();
            let api = match build_local_api() {
                Ok(client) => client,
                Err(error) => {
                    eprintln!("Rebind failed: {error}");
                    std::process::exit(1);
                }
            };
            match api
                .post(format!("{api_url}/api/tabs/{tab_id}/session/rebind"))
                .json(&serde_json::json!({"session_id": session_id}))
                .send()
            {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        let message = response.text().unwrap_or_default();
                        eprintln!("Rebind failed: {message}");
                        std::process::exit(1);
                    }
                    let value = response
                        .json::<serde_json::Value>()
                        .unwrap_or_else(|_| serde_json::json!({}));
                    print_rebind_result(json, value);
                }
                Err(error) => {
                    eprintln!("Rebind failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        TabAction::Repair {
            provider,
            session,
            from_evidence,
            from_journal,
            tab,
        } => handle_tab_repair(
            client,
            json,
            provider,
            session,
            from_evidence,
            from_journal,
            tab,
        ),
        TabAction::AdoptAgent {
            provider,
            force,
            tab,
        } => handle_tab_adopt_agent(client, json, provider, force, tab),
        TabAction::Stop { tab } => {
            match tab.as_deref().map(classify_remote_tab_id) {
                Some(RemoteTabTarget::Remote(peer, remote_tab)) => {
                    let path = format!("/api/tabs/{}/stop", urlencoding::encode(&remote_tab));
                    handle_result(json, Ok(relay_tab_json(&peer, "POST", &path, Some("{}"))));
                    return;
                }
                Some(RemoteTabTarget::Invalid) => {
                    exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
                }
                _ => {}
            }
            let tab_id = resolve_tab(client, tab);
            let (api_url, _) = read_api_config();
            let api = match build_local_api() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to connect to local API: {}", e);
                    std::process::exit(1);
                }
            };
            match api
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
            if let Some((peer, remote_tab)) = tab.as_deref().and_then(parse_remote_tab_id) {
                let path = remote_tab_chat_path(&remote_tab, since);
                let data = relay_tab_json(&peer, "GET", &path, None);
                if json {
                    handle_result(true, Ok(data));
                } else {
                    print_tab_chat(&data);
                }
                return;
            }
            if tab
                .as_deref()
                .is_some_and(|id| matches!(classify_remote_tab_id(id), RemoteTabTarget::Invalid))
            {
                exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
            }
            let tab_id = resolve_tab(client, tab);
            let (api_url, _) = read_api_config();
            let api = match build_local_api() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to connect to local API: {}", e);
                    std::process::exit(1);
                }
            };
            let url = format!("{}/api/tabs/{}/chat?since={}", api_url, tab_id, since);
            match api.get(url).send() {
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

fn status_prompt_lines(status: &serde_json::Value) -> Vec<String> {
    let Some(prompt) = status
        .get("pending_prompt")
        .filter(|value| !value.is_null())
    else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if let Some(question) = prompt.get("question").and_then(|value| value.as_str()) {
        lines.push(format!("prompt:  {question}"));
    }
    if let Some(options) = prompt.get("options").and_then(|value| value.as_array()) {
        for (index, option) in options.iter().enumerate() {
            let Some(label) = option.get("label").and_then(|value| value.as_str()) else {
                continue;
            };
            lines.push(format!("  {}. {}", index + 1, label));
        }
    }
    lines
}

fn print_rebind_result(json: bool, value: serde_json::Value) {
    if json {
        print_json(&value);
        return;
    }
    let tab = value.get("tab_id").and_then(|v| v.as_str()).unwrap_or("-");
    let found = value
        .get("found_session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let requested = value
        .get("requested_session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let changed = value
        .get("changed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    println!(
        "Tab {tab}: found session {found}; requested session {requested}; {}",
        if changed { "rebound" } else { "already bound" }
    );
}

fn tab_rpc(
    client: &mut dyn RpcClient,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, CliError> {
    client
        .call_detailed(method, Some(params))
        .map_err(CliError::from)
}

fn focus_tab(client: &mut dyn RpcClient, id: &str) -> Result<serde_json::Value, CliError> {
    tab_rpc(client, "tab.focus", serde_json::json!({ "tab_id": id }))
}

fn close_tab(
    client: &mut dyn RpcClient,
    params: serde_json::Value,
) -> Result<serde_json::Value, CliError> {
    tab_rpc(client, "tab.close", params)
}

fn rename_tab(
    client: &mut dyn RpcClient,
    tab_id: &str,
    title: &str,
) -> Result<serde_json::Value, CliError> {
    tab_rpc(
        client,
        "tab.rename",
        serde_json::json!({ "tab_id": tab_id, "title": title }),
    )
}

fn describe_tab(
    client: &mut dyn RpcClient,
    tab_id: &str,
    description: &str,
) -> Result<serde_json::Value, CliError> {
    tab_rpc(
        client,
        "tab.describe",
        serde_json::json!({ "tab_id": tab_id, "description": description }),
    )
}

fn tab_list_params(
    workspace_id: Option<String>,
    all_workspaces: bool,
    agents_only: bool,
) -> serde_json::Value {
    serde_json::json!({
        "workspace_id": workspace_id,
        "focused_workspace": false,
        "all_workspaces": all_workspaces,
        "agents_only": agents_only,
    })
}

fn tab_search_params() -> serde_json::Value {
    tab_list_params(None, true, false)
}

fn fetch_search_tabs(client: &mut dyn RpcClient) -> Result<serde_json::Value, String> {
    client.call("tab.list", Some(tab_search_params()))
}

fn validate_user_tab_selectors(action: &TabAction) -> Result<(), String> {
    fn invalid(value: &str) -> bool {
        matches!(classify_remote_tab_id(value), RemoteTabTarget::Invalid)
    }
    let invalid_selector = match action {
        TabAction::Inspect { tabs, tab, .. } => tabs
            .iter()
            .chain(tab)
            .find(|value| invalid(value))
            .map(String::as_str),
        TabAction::Focus { id, tab } | TabAction::Close { id, tab } => id
            .as_deref()
            .into_iter()
            .chain(tab.as_deref())
            .find(|value| invalid(value)),
        TabAction::Restart { tab, .. }
        | TabAction::Rename { tab, .. }
        | TabAction::Describe { tab, .. }
        | TabAction::GetText { tab, .. }
        | TabAction::Exec { tab, .. }
        | TabAction::Status { tab }
        | TabAction::Respond { tab, .. }
        | TabAction::AutoConfirm { tab, .. }
        | TabAction::Rebind { tab, .. }
        | TabAction::Repair { tab, .. }
        | TabAction::AdoptAgent { tab, .. }
        | TabAction::Stop { tab }
        | TabAction::Chat { tab, .. } => tab.as_deref().filter(|value| invalid(value)),
        TabAction::Capture { action } => match action {
            crate::args::misc::TabCaptureAction::Start { tab, .. }
            | crate::args::misc::TabCaptureAction::Stop { tab } => {
                tab.as_deref().filter(|value| invalid(value))
            }
        },
        _ => None,
    };
    invalid_selector
        .map(|selector| Err(format!("{INVALID_REMOTE_TAB_ID}: {selector}")))
        .unwrap_or(Ok(()))
}

fn ordinary_tab_list_path(params: &serde_json::Value) -> String {
    let mut query = vec![];
    if let Some(workspace) = params
        .get("workspace_id")
        .and_then(serde_json::Value::as_str)
    {
        query.push(format!("workspace_id={workspace}"));
    }
    if params
        .get("all_workspaces")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        query.push("all_workspaces=true".into());
    }
    if params
        .get("agents_only")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        query.push("agents=true".into());
    }
    if query.is_empty() {
        "/api/tabs".into()
    } else {
        format!("/api/tabs?{}", query.join("&"))
    }
}

fn fetch_ordinary_tabs(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let (base, _) = read_api_config();
    let http = build_local_api().map_err(|error| error.to_string())?;
    let response = http
        .get(format!("{base}{}", ordinary_tab_list_path(params)))
        .send()
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let text = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {}: {text}", status.as_u16()));
    }
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

pub(super) fn relay_tab_json(
    peer: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> serde_json::Value {
    crate::mesh_cli::mesh_relay_fetch_json(peer, method, path, body)
}

#[cfg(test)]
mod tests;
