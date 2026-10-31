use std::io::Write;

use crate::args::misc::WorkspaceAction;
use crate::client::{RpcClient, SocketClient};
use crate::cmd::print::{exit_with_cli_error, write_cli_error, CliError, CLI_ERROR_EXIT_CODE};

const NO_WORKSPACE_IDENTITY: &str =
    "no workspace identity: pass --workspace <id>, run inside a Domios tab where \
     CODETERM_TAB_ID is set, or focus a workspace";

const NO_TAB_IDENTITY: &str =
    "pass --tab <id> or run inside a Domios tab where CODETERM_TAB_ID is set. \
     There is no focused-tab fallback: a caller that cannot prove which tab it is \
     refuses rather than acting on another tab's behalf";

fn workspace_close_params(workspace_id: &str, force: bool) -> serde_json::Value {
    serde_json::json!({
        "workspace_id": workspace_id,
        "force": force,
        "confirm": workspace_id,
    })
}

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
        writeln!(stdout, "{}", crate::cmd::output::encode(value).unwrap())?;
    } else {
        writeln!(stderr, "{}", notice)?;
    }
    Ok(true)
}

pub(crate) fn resolve_tab(_client: &mut SocketClient, id: Option<String>) -> String {
    resolve_caller_tab(id, "this command")
}

/// Caller identity is explicit or honestly absent: an explicit id, then
/// `$CODETERM_TAB_ID` (injected per shell by the CodeTerm PTY), then a refusal.
pub(crate) fn resolve_caller_tab(id: Option<String>, verb: &str) -> String {
    let env_tab_id = crate::nonempty_env("CODETERM_TAB_ID");
    match caller_tab_identity(id.as_deref(), env_tab_id.as_deref()) {
        Some(tab_id) => tab_id,
        None => exit_with_cli_error(json_output_requested(), &no_tab_identity_error(verb)),
    }
}

pub(crate) fn caller_tab_identity(
    explicit: Option<&str>,
    env_tab_id: Option<&str>,
) -> Option<String> {
    trimmed(explicit)
        .or_else(|| trimmed(env_tab_id))
        .map(str::to_string)
}

pub(crate) fn no_tab_identity_error(verb: &str) -> CliError {
    CliError::new(format!(
        "{verb} cannot resolve a caller tab: {NO_TAB_IDENTITY}"
    ))
}

/// The refusal sits below the arg parser at every `resolve_tab` call site, so it
/// reads `--json` from argv rather than threading a flag through all of them.
fn json_output_requested() -> bool {
    std::env::args().any(|arg| arg == "--json")
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub(crate) fn handle_workspace_action(
    client: &mut dyn RpcClient,
    action: WorkspaceAction,
    json: bool,
) -> Option<Result<serde_json::Value, String>> {
    match action {
        WorkspaceAction::List => {
            let workspaces = client.call("workspace.list", None);
            if json || !crate::cmd::output::verbose() || workspaces.is_err() {
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
            let caller_tab_id = crate::nonempty_env("CODETERM_TAB_ID");
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
        WorkspaceAction::Current { workspace } => {
            let result = client.call("workspace.list", None);
            match result {
                Ok(value) => {
                    let empty = Vec::new();
                    let workspaces = value.as_array().unwrap_or(&empty);
                    let caller_tab_id = crate::nonempty_env("CODETERM_TAB_ID");
                    let workspace_env = crate::nonempty_env("CODETERM_WORKSPACE_ID");
                    let identity = WorkspaceIdentity {
                        explicit: workspace.as_deref(),
                        caller_tab_id: caller_tab_id.as_deref(),
                        workspace_env: workspace_env.as_deref(),
                    };
                    let resolved = workspace_current_resolution(workspaces, identity);
                    let notices = workspace_current_notices(workspaces, identity, &resolved);
                    let code = write_workspace_current(
                        resolved,
                        &notices,
                        json,
                        &mut std::io::stdout(),
                        &mut std::io::stderr(),
                    );
                    if code != 0 {
                        std::process::exit(code);
                    }
                    None
                }
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            }
        }
        WorkspaceAction::Create {
            name,
            cwd,
            provision,
        } => {
            let requested_name = name.clone();
            let mut params = serde_json::json!({ "name": name });
            if provision {
                params["provision"] = serde_json::json!(true);
            } else if let Some(cwd) = cwd {
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
        WorkspaceAction::SetCwd { id, path } => Some(client.call(
            "workspace.set_cwd",
            Some(serde_json::json!({"workspace_id": id, "cwd": path})),
        )),
        WorkspaceAction::Rename { id, name } => Some(client.call(
            "workspace.rename",
            Some(serde_json::json!({ "workspace_id": id, "name": name })),
        )),
        WorkspaceAction::Close { id, force } => {
            let mut params = workspace_close_params(&id, force);
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

#[cfg(test)]
pub(crate) fn workspace_current_id<'a>(
    workspaces: &'a [serde_json::Value],
    caller_tab_id: Option<&str>,
) -> Option<&'a str> {
    workspace_current_resolution(workspaces, WorkspaceIdentity::caller_tab(caller_tab_id))
        .ok()
        .map(|resolved| resolved.id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceResolutionSource {
    Explicit,
    CallerTab,
    WorkspaceEnv,
    FocusFallback,
}

impl WorkspaceResolutionSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::CallerTab => "caller-tab",
            Self::WorkspaceEnv => "workspace-env",
            Self::FocusFallback => "focus-fallback",
        }
    }
}

/// Every rung the resolver may consult, passed in rather than read from the
/// process so a fixture can clear all of them at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkspaceIdentity<'a> {
    pub(crate) explicit: Option<&'a str>,
    pub(crate) caller_tab_id: Option<&'a str>,
    pub(crate) workspace_env: Option<&'a str>,
}

#[cfg(test)]
impl<'a> WorkspaceIdentity<'a> {
    pub(crate) fn caller_tab(caller_tab_id: Option<&'a str>) -> Self {
        Self {
            caller_tab_id,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceResolution<'a> {
    pub(crate) id: &'a str,
    pub(crate) source: WorkspaceResolutionSource,
}

/// Explicit flag, then the daemon-owned `CODETERM_TAB_ID` membership join, then a
/// validated `CODETERM_WORKSPACE_ID`, then the active workspace. Never positional.
pub(crate) fn workspace_current_resolution<'a>(
    workspaces: &'a [serde_json::Value],
    identity: WorkspaceIdentity<'_>,
) -> Result<WorkspaceResolution<'a>, CliError> {
    if let Some(explicit) = trimmed(identity.explicit) {
        return workspace_by_id(workspaces, explicit)
            .map(|id| WorkspaceResolution {
                id,
                source: WorkspaceResolutionSource::Explicit,
            })
            .ok_or_else(|| CliError::new(format!("workspace '{explicit}' not found")));
    }
    if let Some(id) = workspace_for_tab(workspaces, identity.caller_tab_id) {
        return Ok(WorkspaceResolution {
            id,
            source: WorkspaceResolutionSource::CallerTab,
        });
    }
    if let Some(id) =
        trimmed(identity.workspace_env).and_then(|env| workspace_by_id(workspaces, env))
    {
        return Ok(WorkspaceResolution {
            id,
            source: WorkspaceResolutionSource::WorkspaceEnv,
        });
    }
    active_workspace_id(workspaces)
        .map(|id| WorkspaceResolution {
            id,
            source: WorkspaceResolutionSource::FocusFallback,
        })
        .ok_or_else(|| CliError::new(NO_WORKSPACE_IDENTITY))
}

/// A rung that was consulted and rejected is announced; the machine-facing
/// signal is the `source` discriminant, not this prose.
pub(crate) fn workspace_current_notices(
    workspaces: &[serde_json::Value],
    identity: WorkspaceIdentity<'_>,
    resolved: &Result<WorkspaceResolution<'_>, CliError>,
) -> Vec<String> {
    let mut notices = Vec::new();
    if identity.explicit.is_some() {
        return notices;
    }
    let used_caller_tab = matches!(resolved, Ok(resolution) if resolution.source == WorkspaceResolutionSource::CallerTab);
    if let Some(env) = trimmed(identity.workspace_env) {
        if !used_caller_tab && workspace_by_id(workspaces, env).is_none() {
            notices.push(format!(
                "warning: CODETERM_WORKSPACE_ID '{env}' is not a known workspace; ignored"
            ));
        }
    }
    if identity.caller_tab_id.is_some() && !used_caller_tab {
        notices.push(
            "warning: caller Tab is absent from the workspace map; identity fell through to the workspace env or the active workspace".to_string(),
        );
    }
    notices
}

pub(crate) fn write_workspace_current<W: Write, E: Write>(
    resolved: Result<WorkspaceResolution<'_>, CliError>,
    notices: &[String],
    json: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> i32 {
    for notice in notices {
        let _ = writeln!(stderr, "{notice}");
    }
    match resolved {
        Ok(resolution) => {
            if json {
                let _ = writeln!(
                    stdout,
                    "{}",
                    crate::cmd::output::encode(&serde_json::json!({
                        "id": resolution.id,
                        "source": resolution.source.as_str()
                    }))
                    .expect("workspace resolution is serializable")
                );
            } else {
                let _ = writeln!(stdout, "{}", resolution.id);
            }
            0
        }
        Err(error) => {
            write_cli_error(json, &error, stdout, stderr);
            CLI_ERROR_EXIT_CODE
        }
    }
}

fn workspace_by_id<'a>(workspaces: &'a [serde_json::Value], id: &str) -> Option<&'a str> {
    workspaces
        .iter()
        .filter_map(|workspace| workspace.get("id").and_then(|value| value.as_str()))
        .find(|candidate| *candidate == id)
}

fn active_workspace_id(workspaces: &[serde_json::Value]) -> Option<&str> {
    workspaces
        .iter()
        .find(|workspace| {
            workspace.get("is_active").and_then(|value| value.as_bool()) == Some(true)
        })?
        .get("id")?
        .as_str()
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
    every: Option<String>,
    until: Option<String>,
    count: Option<u32>,
    require_agent: bool,
    to_shell: bool,
    system_context: Option<String>,
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
        every,
        until,
        count,
        require_agent,
        to_shell,
        system_context,
    );
}

#[cfg(test)]
mod tests;
