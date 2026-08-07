use super::tab_restart::handle_shell_restart;
use crate::client::{RpcClient, SocketClient};
use crate::cmd::agent::normalize_tab_description;
use crate::cmd::print::{
    exit_with_cli_error, handle_cli_result, handle_result, print_json, tab_list_json_envelope,
    CliError,
};
use crate::{build_local_api, read_api_config, resolve_tab, TabAction};

pub(crate) const INVALID_REMOTE_TAB_ID: &str = "invalid remote tab id";

mod capture;
mod chat_view;
mod inspect;
mod render;
pub(crate) mod remote_target;
mod search;
mod team_utility;
use capture::handle_tab_capture;
use chat_view::print_tab_chat;
use inspect::handle_tab_inspect;
#[cfg(test)]
use render::format_tab_row;
pub use render::{render_tab_tree, render_tab_tree_with_current};
#[cfg(test)]
use search::rank_tabs;
pub use search::print_tab_find_results;
#[cfg(test)]
use remote_target::remote_inspect_target;
use remote_target::{
    classify_remote_tab_id, parse_remote_tab_id, remote_tab_chat_path, RemoteTabTarget,
};

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
            let caller_tab_id = std::env::var("CODETERM_TAB_ID").ok();
            let workspace_id = if all_workspaces {
                None
            } else {
                workspace
                    .or_else(|| std::env::var("CODETERM_WORKSPACE_ID").ok())
                    .or_else(|| caller_workspace_id(client, caller_tab_id.as_deref()))
            };
            let params = tab_list_params(workspace_id, all_workspaces, agents, caller_tab_id);
            match fetch_ordinary_tabs(&params) {
                Ok(value) => {
                    let Some(arr) = value.as_array() else {
                        handle_result(json, Ok(value));
                        return;
                    };
                    let list = arr.clone();
                    if json {
                        print_json(&tab_list_json_envelope(list.clone()));
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
            if let Some(team_id) = team.as_deref() {
                let creator_tab_id = std::env::var("CODETERM_TAB_ID")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| {
                        eprintln!("Error: --team requires CODETERM_TAB_ID from a live Team member");
                        std::process::exit(1);
                    });
                body["team_id"] = serde_json::Value::String(team_id.to_string());
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
        "all_workspaces": all_workspaces,
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
            Some(serde_json::json!({ "tab_ids": [tab_id], "lifecycle": false })),
        )
        .ok()?
        .as_array()?
        .first()?
        .get("workspace_id")?
        .as_str()
        .map(str::to_string)
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
    format!("/api/tabs?{}", query.join("&"))
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
