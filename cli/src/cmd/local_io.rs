use std::io::Write;

use crate::args::misc::WorkspaceAction;
use crate::client::{RpcClient, SocketClient};

pub(crate) fn workspace_create_cwd_hint() -> String {
    "hint: no --cwd — agents in this workspace inherit the daemon working directory; pass --cwd <project dir> for project work".to_string()
}

pub(crate) fn workspace_reuse_notice(
    value: &serde_json::Value,
    requested_name: &str,
) -> Option<String> {
    if value.get("existing").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let ws_name = value.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let ws_id = value.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    Some(format!(
        "reused existing workspace {} '{}' (same cwd); requested name '{}' ignored",
        ws_id, ws_name, requested_name
    ))
}

pub(crate) fn write_workspace_create_reuse_output<W: Write, E: Write>(
    value: &serde_json::Value,
    requested_name: &str,
    json: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> std::io::Result<bool> {
    let Some(notice) = workspace_reuse_notice(value, requested_name) else {
        return Ok(false);
    };
    if json {
        writeln!(stdout, "{}", serde_json::to_string_pretty(value).unwrap())?;
    } else {
        writeln!(stderr, "{}", notice)?;
    }
    Ok(true)
}

pub(crate) fn resolve_tab(client: &mut SocketClient, id: Option<String>) -> String {
    if let Some(id) = id {
        return id;
    }
    // 2. $CODETERM_TAB_ID env var (set by CodeTerm PTY for each shell)
    if let Ok(env_id) = std::env::var("CODETERM_TAB_ID") {
        if !env_id.is_empty() {
            return env_id;
        }
    }
    match client.call("tab.list", None) {
        Ok(value) => {
            if let Some(arr) = value.as_array() {
                let tab = arr
                    .iter()
                    .find(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                    .or_else(|| arr.first());
                if let Some(id) = tab.and_then(|p| p.get("id").and_then(|v| v.as_str())) {
                    return id.to_string();
                }
            }
            eprintln!("Error: no Tabs found");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

pub(crate) fn handle_workspace_action(
    client: &mut dyn RpcClient,
    action: WorkspaceAction,
    json: bool,
) -> Option<Result<serde_json::Value, String>> {
    match action {
        WorkspaceAction::List => {
            let workspaces = client.call("workspace.list", None);
            if json || workspaces.is_err() {
                return Some(workspaces);
            }
            let workspaces = workspaces.expect("checked above");
            let tabs = optional_overview_call(
                client,
                "tab.list",
                Some(serde_json::json!({ "focused_workspace": false })),
            );
            let teams = optional_overview_call(client, "team.list", None);
            let plans = optional_overview_call(
                client,
                "plan.list",
                Some(serde_json::json!({ "status": "active" })),
            );
            let caller_tab_id = std::env::var("CODETERM_TAB_ID").ok();
            let caller_workspace_id = workspaces
                .as_array()
                .and_then(|rows| workspace_for_tab(rows, caller_tab_id.as_deref()));
            print!(
                "{}",
                super::workspace_map::render(
                    &workspaces,
                    tabs.as_ref(),
                    teams.as_ref(),
                    plans.as_ref(),
                    caller_workspace_id,
                    caller_tab_id.as_deref(),
                )
            );
            None
        }
        WorkspaceAction::Current => {
            let result = client.call("workspace.list", None);
            match result {
                Ok(value) => {
                    if let Some(arr) = value.as_array() {
                        let caller_tab_id = std::env::var("CODETERM_TAB_ID").ok();
                        let resolution =
                            workspace_current_resolution(arr, caller_tab_id.as_deref());
                        if caller_tab_id.is_some()
                            && resolution.as_ref().is_some_and(|resolved| {
                                resolved.source == WorkspaceResolutionSource::FocusFallback
                            })
                        {
                            eprintln!(
                                "warning: caller Tab is absent from the workspace map; using the focused workspace if available"
                            );
                        }
                        if let Some(resolved) = resolution {
                            if json {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&serde_json::json!({
                                        "id": resolved.id,
                                        "source": resolved.source.as_str()
                                    }))
                                    .expect("workspace resolution is serializable")
                                );
                            } else {
                                println!("{}", resolved.id);
                            }
                        }
                    }
                    None
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        WorkspaceAction::Create { name, cwd } => {
            let requested_name = name.clone();
            let mut params = serde_json::json!({ "name": name });
            if let Some(cwd) = cwd {
                params["cwd"] = serde_json::json!(cwd);
            } else {
                eprintln!("{}", workspace_create_cwd_hint());
            }
            match client.call("workspace.create", Some(params)) {
                Ok(val) if workspace_reuse_notice(&val, &requested_name).is_some() => {
                    let _ = write_workspace_create_reuse_output(
                        &val,
                        &requested_name,
                        json,
                        &mut std::io::stdout(),
                        &mut std::io::stderr(),
                    );
                    None
                }
                result => Some(result),
            }
        }
        WorkspaceAction::Focus { id } => Some(client.call(
            "workspace.focus",
            Some(serde_json::json!({ "workspace_id": id })),
        )),
        WorkspaceAction::Rename { id, name } => Some(client.call(
            "workspace.rename",
            Some(serde_json::json!({ "workspace_id": id, "name": name })),
        )),
        WorkspaceAction::Close { id } => {
            let mut params = serde_json::json!({ "workspace_id": id });
            crate::cmd::plan::caller_context::CallerContext::from_env()
                .apply_caller_tab(&mut params);
            Some(client.call("workspace.close", Some(params)))
        }
        WorkspaceAction::MoveTab { tab_id, from, to } => Some(client.call(
            "workspace.move_tab",
            Some(serde_json::json!({
                "tab_id": tab_id,
                "from_workspace_id": from,
                "to_workspace_id": to
            })),
        )),
    }
}

fn workspace_for_tab<'a>(
    workspaces: &'a [serde_json::Value],
    tab_id: Option<&str>,
) -> Option<&'a str> {
    let tab_id = tab_id?.trim();
    if tab_id.is_empty() {
        return None;
    }
    workspaces
        .iter()
        .find(|workspace| {
            workspace
                .get("tab_ids")
                .and_then(|value| value.as_array())
                .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(tab_id)))
        })?
        .get("id")?
        .as_str()
}

fn optional_overview_call(
    client: &mut dyn RpcClient,
    method: &str,
    params: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    match client.call(method, params) {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("warning: workspace overview omitted {method}: {error}");
            None
        }
    }
}

pub(crate) fn workspace_current_id<'a>(
    workspaces: &'a [serde_json::Value],
    caller_tab_id: Option<&str>,
) -> Option<&'a str> {
    workspace_current_resolution(workspaces, caller_tab_id).map(|resolved| resolved.id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceResolutionSource {
    CallerTab,
    FocusFallback,
}

impl WorkspaceResolutionSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::CallerTab => "caller-tab",
            Self::FocusFallback => "focus-fallback",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceResolution<'a> {
    pub(crate) id: &'a str,
    pub(crate) source: WorkspaceResolutionSource,
}

pub(crate) fn workspace_current_resolution<'a>(
    workspaces: &'a [serde_json::Value],
    caller_tab_id: Option<&str>,
) -> Option<WorkspaceResolution<'a>> {
    if let Some(id) = workspace_for_tab(workspaces, caller_tab_id) {
        return Some(WorkspaceResolution {
            id,
            source: WorkspaceResolutionSource::CallerTab,
        });
    }
    let id = workspaces
        .iter()
        .find(|workspace| {
            workspace.get("is_active").and_then(|value| value.as_bool()) == Some(true)
        })?
        .get("id")?
        .as_str()?;
    Some(WorkspaceResolution {
        id,
        source: WorkspaceResolutionSource::FocusFallback,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "CLI wrapper preserves send text argument surface."
)]
pub(crate) fn handle_send_text(
    client: &mut SocketClient,
    json: bool,
    tab: Option<String>,
    text: Option<String>,
    from_file: Option<String>,
    from_pad: Option<String>,
    stdin: bool,
    actions: Option<String>,
    delay: Option<String>,
    at: Option<String>,
    require_agent: bool,
    to_shell: bool,
) {
    crate::cmd::send_text::handle_send_text(
        client,
        json,
        tab,
        text,
        from_file,
        from_pad,
        stdin,
        actions,
        delay,
        at,
        require_agent,
        to_shell,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::SocketCallError;

    struct JsonWorkspaceClient {
        calls: Vec<String>,
        response: serde_json::Value,
    }

    impl RpcClient for JsonWorkspaceClient {
        fn call_detailed(
            &mut self,
            method: &str,
            _params: Option<serde_json::Value>,
        ) -> Result<serde_json::Value, SocketCallError> {
            self.calls.push(method.to_string());
            Ok(self.response.clone())
        }
    }

    #[test]
    fn workspace_cwd_hint_names_the_flag() {
        let hint = workspace_create_cwd_hint();
        assert!(hint.contains("--cwd"));
        assert!(hint.contains("daemon working directory"));
    }

    #[test]
    fn workspace_list_json_keeps_the_workspace_only_contract() {
        let response = serde_json::json!([{
            "id": "ws_one",
            "name": "one",
            "tab_ids": ["tab_one"]
        }]);
        let mut client = JsonWorkspaceClient {
            calls: Vec::new(),
            response: response.clone(),
        };

        let result = handle_workspace_action(&mut client, WorkspaceAction::List, true)
            .expect("JSON mode returns the raw result")
            .unwrap();

        assert_eq!(result, response);
        assert_eq!(client.calls, ["workspace.list"]);
    }
}
