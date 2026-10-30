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
fn workspace_close_params_echo_the_target_without_changing_force_semantics() {
    assert_eq!(
        workspace_close_params("workspace-1", false),
        serde_json::json!({
            "workspace_id": "workspace-1",
            "force": false,
            "confirm": "workspace-1",
        })
    );
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

/// `agent ack` writes ownership on behalf of an identity, so its arm must
/// reach the explicit-only resolver and never the client-based wrapper.
#[test]
fn agent_ack_resolves_its_tab_from_explicit_identity_only() {
    const AGENT_RS: &str = include_str!("../agent.rs");
    let arm = AGENT_RS
        .split_once("AgentAction::Ack {")
        .expect("the ack arm exists")
        .1
        .split_once("AgentAction::")
        .expect("the ack arm ends at the next action")
        .0;

    assert!(
        arm.contains("resolve_caller_tab(tab, \"agent ack\")"),
        "{arm}"
    );
    assert!(
        !arm.contains("resolve_tab("),
        "ack must not resolve through the client-based wrapper: {arm}"
    );
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
    assert_eq!(payload["result"]["id"], "ws_active");
    assert_eq!(payload["result"]["source"], "focus-fallback");
    assert!(
        !stderr.is_empty(),
        "an unused caller tab is announced; no workspace env is set here, so this is that warning"
    );
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
