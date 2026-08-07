use std::io::Write;

use crate::args::misc::WorkspaceAction;
use crate::client::{RpcClient, SocketClient};
use crate::cmd::print::{exit_with_cli_error, write_cli_error, CliError, CLI_ERROR_EXIT_CODE};

const NO_WORKSPACE_IDENTITY: &str =
    "no workspace identity: pass --workspace <id>, run inside a CodeTerm tab where \
     CODETERM_TAB_ID is set, or focus a workspace";

const NO_TAB_IDENTITY: &str =
    "pass --tab <id> or run inside a CodeTerm tab where CODETERM_TAB_ID is set. \
     There is no focused-tab fallback: a caller that cannot prove which tab it is \
     refuses rather than acting on another tab's behalf";

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
    let used_caller_tab =
        matches!(resolved, Ok(resolution) if resolution.source == WorkspaceResolutionSource::CallerTab);
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
                    serde_json::to_string_pretty(&serde_json::json!({
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

    /// Every rung `resolve_caller_tab` reads is a parameter here, so the
    /// no-identity fixture clears all of them without racing on process env.
    #[test]
    fn caller_tab_identity_prefers_the_explicit_id_over_the_environment() {
        assert_eq!(
            caller_tab_identity(Some("tab_explicit"), Some("tab_env")),
            Some("tab_explicit".to_string())
        );
    }

    #[test]
    fn caller_tab_identity_falls_back_to_the_environment_only_without_an_explicit_id() {
        assert_eq!(
            caller_tab_identity(None, Some("tab_env")),
            Some("tab_env".to_string())
        );
        assert_eq!(
            caller_tab_identity(Some("   "), Some("tab_env")),
            Some("tab_env".to_string()),
            "a blank explicit id is not an identity"
        );
    }

    #[test]
    fn caller_tab_identity_is_absent_when_no_rung_proves_it() {
        assert_eq!(caller_tab_identity(None, None), None);
        assert_eq!(caller_tab_identity(Some(" "), Some("")), None);
    }

    #[test]
    fn no_tab_identity_refusal_names_the_verb_and_leaves_stdout_empty() {
        let error = no_tab_identity_error("agent ack");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        crate::cmd::print::write_cli_error(false, &error, &mut stdout, &mut stderr);

        assert!(error.message.starts_with("agent ack"), "{}", error.message);
        assert!(
            stdout.is_empty(),
            "a refused identity must not print a tab id: {}",
            String::from_utf8_lossy(&stdout)
        );
        assert!(!stderr.is_empty());
        assert_ne!(crate::cmd::print::CLI_ERROR_EXIT_CODE, 0);
    }

    #[test]
    fn no_tab_identity_refusal_is_a_json_envelope_under_json() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        crate::cmd::print::write_cli_error(
            true,
            &no_tab_identity_error("agent ack"),
            &mut stdout,
            &mut stderr,
        );

        let envelope: serde_json::Value =
            serde_json::from_slice(&stdout).expect("stdout carries one JSON envelope");
        assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
        assert!(stderr.is_empty());
    }

    #[test]
    fn workspace_cwd_hint_names_the_flag() {
        let hint = workspace_create_cwd_hint();
        assert!(hint.contains("--cwd"));
        assert!(hint.contains("daemon working directory"));
    }

    /// Every later rung is satisfiable and points at a different id, so only a
    /// real short-circuit on the earlier rung can produce the asserted value.
    fn ladder_fixture() -> serde_json::Value {
        serde_json::json!([
            {"id": "ws_explicit", "is_active": false, "tab_ids": []},
            {"id": "ws_caller", "is_active": false, "tab_ids": ["self_tab"]},
            {"id": "ws_env", "is_active": false, "tab_ids": []},
            {"id": "ws_active", "is_active": true, "tab_ids": []}
        ])
    }

    fn rendered_current(
        workspaces: &serde_json::Value,
        identity: WorkspaceIdentity<'_>,
        json: bool,
    ) -> (i32, String, String) {
        let rows = workspaces.as_array().expect("fixture is an array");
        let resolved = workspace_current_resolution(rows, identity);
        let notices = workspace_current_notices(rows, identity, &resolved);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = write_workspace_current(resolved, &notices, json, &mut stdout, &mut stderr);
        (
            code,
            String::from_utf8(stdout).expect("stdout is utf8"),
            String::from_utf8(stderr).expect("stderr is utf8"),
        )
    }

    #[test]
    fn workspace_resolution_takes_the_explicit_flag_over_every_other_rung() {
        let workspaces = ladder_fixture();

        let resolved = workspace_current_resolution(
            workspaces.as_array().unwrap(),
            WorkspaceIdentity {
                explicit: Some("ws_explicit"),
                caller_tab_id: Some("self_tab"),
                workspace_env: Some("ws_env"),
            },
        )
        .unwrap();

        assert_eq!(resolved.id, "ws_explicit");
        assert_eq!(resolved.source.as_str(), "explicit");
    }

    #[test]
    fn workspace_resolution_refuses_an_explicit_id_the_daemon_does_not_know() {
        let workspaces = ladder_fixture();

        let error = workspace_current_resolution(
            workspaces.as_array().unwrap(),
            WorkspaceIdentity {
                explicit: Some("ws_ghost"),
                caller_tab_id: Some("self_tab"),
                workspace_env: Some("ws_env"),
            },
        )
        .expect_err("an unknown explicit id never falls through to a later rung");

        assert!(error.message.contains("ws_ghost"), "{}", error.message);
    }

    #[test]
    fn workspace_resolution_prefers_the_tab_join_over_the_workspace_env() {
        let workspaces = ladder_fixture();

        let resolved = workspace_current_resolution(
            workspaces.as_array().unwrap(),
            WorkspaceIdentity {
                explicit: None,
                caller_tab_id: Some("self_tab"),
                workspace_env: Some("ws_env"),
            },
        )
        .unwrap();

        assert_eq!(resolved.id, "ws_caller");
        assert_eq!(resolved.source.as_str(), "caller-tab");
    }

    /// The caller tab is absent from every `tab_ids`, so rung 2 cannot answer and
    /// the active workspace is a different id — a wrong result stays visible.
    #[test]
    fn workspace_resolution_uses_the_workspace_env_only_when_the_tab_join_is_empty() {
        let workspaces = ladder_fixture();

        let resolved = workspace_current_resolution(
            workspaces.as_array().unwrap(),
            WorkspaceIdentity {
                explicit: None,
                caller_tab_id: Some("ghost_tab"),
                workspace_env: Some("ws_env"),
            },
        )
        .unwrap();

        assert_eq!(resolved.id, "ws_env");
        assert_eq!(resolved.source.as_str(), "workspace-env");
    }

    #[test]
    fn workspace_resolution_ignores_a_workspace_env_the_daemon_does_not_know() {
        let workspaces = ladder_fixture();
        let identity = WorkspaceIdentity {
            explicit: None,
            caller_tab_id: None,
            workspace_env: Some("ws_ghost"),
        };

        let (code, stdout, stderr) = rendered_current(&workspaces, identity, false);

        assert_eq!(code, 0);
        assert_eq!(stdout, "ws_active\n");
        assert!(stderr.contains("ws_ghost"), "{stderr}");
    }

    /// No workspace carries `is_active`, so rung 4 cannot answer either — a
    /// fixture with an active workspace would return early and never reach this.
    #[test]
    fn workspace_current_without_any_identity_exits_one_with_empty_stdout() {
        let workspaces = serde_json::json!([
            {"id": "ws_one", "is_active": false, "tab_ids": []},
            {"id": "ws_two", "is_active": false, "tab_ids": []}
        ]);
        let identity = WorkspaceIdentity {
            explicit: None,
            caller_tab_id: None,
            workspace_env: None,
        };

        let (code, stdout, stderr) = rendered_current(&workspaces, identity, false);

        assert_eq!(code, CLI_ERROR_EXIT_CODE);
        assert_ne!(code, 0);
        assert!(
            stdout.is_empty(),
            "an unresolved identity must not print a workspace id: {stdout}"
        );
        assert!(stderr.starts_with("Error: "), "{stderr}");
    }

    #[test]
    fn workspace_current_without_any_identity_emits_a_json_envelope() {
        let workspaces = serde_json::json!([{"id": "ws_one", "is_active": false, "tab_ids": []}]);
        let identity = WorkspaceIdentity {
            explicit: None,
            caller_tab_id: None,
            workspace_env: None,
        };

        let (code, stdout, stderr) = rendered_current(&workspaces, identity, true);

        assert_eq!(code, CLI_ERROR_EXIT_CODE);
        let envelope: serde_json::Value =
            serde_json::from_str(&stdout).expect("stdout carries one JSON envelope");
        assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
        assert!(stderr.is_empty(), "{stderr}");
    }

    #[test]
    fn workspace_current_marks_the_active_workspace_fallback_for_machines() {
        let workspaces = ladder_fixture();
        let identity = WorkspaceIdentity {
            explicit: None,
            caller_tab_id: Some("ghost_tab"),
            workspace_env: None,
        };

        let (code, stdout, stderr) = rendered_current(&workspaces, identity, true);

        assert_eq!(code, 0);
        let payload: serde_json::Value = serde_json::from_str(&stdout).expect("json payload");
        assert_eq!(payload["id"], "ws_active");
        assert_eq!(payload["source"], "focus-fallback");
        assert!(stderr.contains("caller Tab is absent"), "{stderr}");
    }

    /// The warning predicate needs both halves: without a caller tab there is
    /// nothing absent from the map, so the fallback is silent.
    #[test]
    fn workspace_current_does_not_warn_about_an_absent_caller_tab_outside_a_tab() {
        let workspaces = ladder_fixture();
        let identity = WorkspaceIdentity {
            explicit: None,
            caller_tab_id: None,
            workspace_env: None,
        };

        let (code, stdout, stderr) = rendered_current(&workspaces, identity, false);

        assert_eq!(code, 0);
        assert_eq!(stdout, "ws_active\n");
        assert!(stderr.is_empty(), "{stderr}");
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
