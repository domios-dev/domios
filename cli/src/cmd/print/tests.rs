use super::*;
use crate::client::RpcError;
use serde_json::json;

/// Every JSON-RPC code the daemon uses starts with this; asserting its
/// absence is structural, unlike pinning a message phrase.
const RPC_CODE_MARKER: &str = "-32";

fn tab_not_found() -> SocketCallError {
    SocketCallError::Rpc(RpcError {
        code: -32603,
        message: "one or more requested Tabs were not found".into(),
        data: None,
    })
}

fn rendered(json: bool, error: &CliError) -> (String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    write_cli_error(json, error, &mut out, &mut err);
    (
        String::from_utf8(out).expect("stdout is utf8"),
        String::from_utf8(err).expect("stderr is utf8"),
    )
}

#[test]
fn refusal_detection_covers_false_results_and_typed_refusal_discriminants() {
    assert!(is_cli_refusal(&json!({"ok": false, "error": "refused"})));
    assert!(is_cli_refusal(&json!({
        "ok": true,
        "data": {"type": "attempt_binding_restore_refused"}
    })));
    assert!(!is_cli_refusal(&json!({"ok": true, "kind": "task"})));
}

#[test]
fn json_error_envelope_goes_to_stdout_and_leaves_stderr_empty() {
    let (out, err) = rendered(true, &CliError::from(tab_not_found()));

    assert!(
        err.is_empty(),
        "--json puts the whole failure on stdout: {err}"
    );
    let envelope: serde_json::Value =
        serde_json::from_str(&out).expect("stdout carries exactly one JSON envelope");
    assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
    assert_eq!(envelope["kind"], "error");
    assert_eq!(envelope["error"]["rpc_code"], -32603);
    assert_eq!(envelope["error"]["code"], "not_found");
}

/// The string path used to own a second, `--json`-blind error arm; the sink
/// exits, so delegation is pinned structurally and rendering behaviourally.
#[test]
fn handle_result_error_path_emits_the_json_envelope_instead_of_bare_stderr() {
    const PRINT_RS: &str = include_str!("../print.rs");
    let start = PRINT_RS
        .find("pub fn handle_result(")
        .expect("handle_result exists");
    let rest = &PRINT_RS[start..];
    let body = &rest[..rest.find("\n}").expect("handle_result body terminates")];

    assert!(
        body.contains("handle_cli_result(json, result.map_err(CliError::new))"),
        "handle_result must reuse the typed sink: {body}"
    );
    assert!(
        !body.contains("eprintln!") && !body.contains("process::exit"),
        "handle_result must not keep a private error path: {body}"
    );

    let (out, err) = rendered(true, &CliError::new("plan not found"));

    assert!(
        err.is_empty(),
        "--json puts the whole failure on stdout: {err}"
    );
    let envelope: serde_json::Value =
        serde_json::from_str(&out).expect("stdout carries exactly one JSON envelope");
    assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
    assert_eq!(envelope["kind"], "error");
    assert_eq!(envelope["error"]["message"], "plan not found");
}

#[test]
fn human_error_goes_to_stderr_without_the_raw_rpc_code() {
    let (out, err) = rendered(false, &CliError::from(tab_not_found()));

    assert!(out.is_empty(), "stdout must stay parseable: {out}");
    assert!(!err.contains(RPC_CODE_MARKER), "{err}");
    assert!(
        err.contains("one or more requested Tabs were not found"),
        "{err}"
    );
}

#[test]
fn error_exit_code_is_non_zero_and_identical_in_both_modes() {
    assert_eq!(cli_error_exit_code(true), cli_error_exit_code(false));
    assert_ne!(cli_error_exit_code(true), 0);
    assert_eq!(cli_error_exit_code(false), CLI_ERROR_EXIT_CODE);
}

#[test]
fn rpc_failure_keeps_the_code_programmatically_and_out_of_the_message() {
    let error = CliError::from(tab_not_found());

    assert_eq!(error.code, Some(-32603));
    assert!(
        !error.message.contains(RPC_CODE_MARKER),
        "{}",
        error.message
    );
}

/// `RpcClient::call` stringifies through `Display`, which is where the code
/// leaks; converted sites must take `call_detailed` instead.
#[test]
fn stringifying_a_call_error_leaks_the_code_that_the_contract_strips() {
    let leaked = tab_not_found().to_string();
    let contracted = CliError::from(tab_not_found());

    assert!(leaked.contains(RPC_CODE_MARKER), "{leaked}");
    assert!(
        !contracted.message.contains(RPC_CODE_MARKER),
        "{}",
        contracted.message
    );
    assert_eq!(contracted.code, Some(-32603));
}

#[test]
fn empty_rpc_message_is_reported_unexplained_instead_of_invented() {
    let error = CliError::from(SocketCallError::Rpc(RpcError {
        code: -32000,
        message: "   ".into(),
        data: None,
    }));

    assert_eq!(error.message, UNEXPLAINED_RPC_FAILURE);
    assert_eq!(error.code, Some(-32000));
}

#[test]
fn transport_failure_has_no_code_and_keeps_its_own_text() {
    let error = CliError::from(SocketCallError::Transport("socket is gone".into()));

    assert_eq!(error.code, None);
    assert_eq!(error.message, "socket is gone");
}

#[test]
fn locally_validated_error_still_emits_the_envelope_with_a_null_code() {
    let (out, err) = rendered(true, &CliError::new("invalid remote tab id"));

    assert!(
        err.is_empty(),
        "--json puts the whole failure on stdout: {err}"
    );
    let envelope: serde_json::Value = serde_json::from_str(&out).expect("one JSON envelope");
    assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
    assert_eq!(envelope["error"]["rpc_code"], serde_json::Value::Null);
    assert_eq!(envelope["error"]["message"], "invalid remote tab id");
}

#[test]
fn tab_list_json_uses_object_envelope_matching_plan_task_list() {
    let enveloped = tab_list_json_envelope(vec![json!({
        "id": "tab_1",
        "title": "one",
        "session_binding_nonce": "nonce-recorded",
        "reported_session_binding_nonce": "nonce-reported",
    })]);
    let enveloped = crate::cmd::output::collection_envelope(&enveloped);
    assert!(
        enveloped.is_object(),
        "tab list --json must be an object envelope, not a bare array"
    );
    let inner = enveloped
        .get("result")
        .expect("envelope carries a result field");
    assert!(inner.is_array(), "tabs field holds the list");
    assert_eq!(inner.as_array().unwrap().len(), 1);
    assert_eq!(inner[0]["session_binding_nonce"], "nonce-recorded");
    assert_eq!(inner[0]["reported_session_binding_nonce"], "nonce-reported");
}

#[test]
fn tab_list_json_envelope_survives_the_printed_uniform_id_pass() {
    let printed = crate::cmd::output::collection_envelope(&tab_list_json_envelope(vec![json!({
        "pane_id": "tab_1",
        "title": "one",
    })]));

    let tabs = printed
        .get("result")
        .expect("the printed value still carries a result field");
    assert!(
        printed.is_object() && tabs.is_array(),
        "print_json must not flatten the envelope back to a bare array: {printed}"
    );
    assert_eq!(tabs[0]["tab_id"], "tab_1");
    assert!(
        printed.get("id").is_none(),
        "the envelope itself must not gain an entity id alias: {printed}"
    );
}

#[test]
fn tab_list_json_envelope_keeps_the_tabs_array_when_empty() {
    let printed = crate::cmd::output::collection_envelope(&tab_list_json_envelope(Vec::new()));

    assert_eq!(printed["result"], json!([]));
}

#[test]
fn tab_list_json_workspace_grouping() {
    let printed = crate::cmd::output::collection_envelope(&tab_list_json_envelope(vec![
        json!({
            "id": "tab_one",
            "workspace_id": "ws_one",
            "workspace_name": "One",
            "scope": "all_workspaces"
        }),
        json!({
            "id": "tab_two",
            "workspace_id": "ws_two",
            "workspace_name": "Two",
            "scope": "all_workspaces"
        }),
    ]));

    assert_eq!(printed["meta"]["scope"]["kind"], "all_workspaces");
    let groups = printed["meta"]["workspaces"]
        .as_array()
        .expect("workspace groups");
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["tabs"].as_array().unwrap().len(), 1);
    assert_eq!(groups[1]["tabs"].as_array().unwrap().len(), 1);
}

#[test]
fn uniform_id_alias_preserves_existing_id() {
    let out = uniform_id_value(&json!({ "id": "canonical", "task_id": "task_1" }));
    assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("canonical"));
}

#[test]
fn uniform_id_alias_adds_top_level_id_for_plan_create() {
    let out = uniform_id_value(&json!({ "plan_id": "plan_1", "title": "Plan" }));
    assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("plan_1"));
    assert_eq!(out.get("plan_id").and_then(|v| v.as_str()), Some("plan_1"));
}

#[test]
fn uniform_id_alias_adds_top_level_id_for_task_add() {
    let out = uniform_id_value(&json!({ "plan_id": "plan_1", "task_id": "task_1" }));
    assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("task_1"));
}

#[test]
fn uniform_id_alias_adds_top_level_id_for_agent_spawn() {
    let out = uniform_id_value(&json!({ "tab_id": "tab_1", "agent": "codex" }));
    assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("tab_1"));
}

#[test]
fn json_output_uses_tab_field_names_recursively() {
    let out = uniform_id_value(&json!({
        "tab_id": "tab_1",
        "parent_tab_id": "tab_0",
        "tabs": [{ "createdByTab": "tab_2" }]
    }));
    assert_eq!(out.get("tab_id").and_then(|v| v.as_str()), Some("tab_1"));
    assert_eq!(
        out.get("parent_tab_id").and_then(|v| v.as_str()),
        Some("tab_0")
    );
    assert_eq!(
        out.pointer("/tabs/0/createdByTab").and_then(|v| v.as_str()),
        Some("tab_2")
    );
}

#[test]
fn json_output_preserves_string_values_named_tab() {
    let input = json!({
        "tab_id": "tab_1",
        "title": "tab",
        "message": "tabs",
        "nested": [{ "body": "tab", "label": "tabs" }]
    });
    let out = uniform_id_value(&input);
    assert_eq!(out["title"], input["title"]);
    assert_eq!(out["message"], input["message"]);
    assert_eq!(out["nested"][0]["body"], input["nested"][0]["body"]);
    assert_eq!(out["nested"][0]["label"], input["nested"][0]["label"]);
}

#[test]
fn json_output_keeps_canonical_tab_key_when_legacy_key_collides() {
    let out = uniform_id_value(&json!({
        "pane_id": "legacy",
        "tab_id": "canonical",
        "createdByPane": "legacy-creator",
        "createdByTab": "canonical-creator"
    }));
    assert!(out.get("pane_id").is_none());
    assert_eq!(out["tab_id"], "canonical");
    assert_eq!(out["id"], "canonical");
    assert!(out.get("createdByPane").is_none());
    assert_eq!(out["createdByTab"], "canonical-creator");
}

#[test]
fn human_output_uses_tab_field_names_without_changing_values() {
    let mut value = serde_json::json!({
        "pane_type": "pane",
        "parentPaneId": "parent-pane"
    });

    rename_tab_json_keys(&mut value);

    assert_eq!(value["tab_type"], "pane");
    assert_eq!(value["parentTabId"], "parent-pane");
    assert!(value.get("pane_type").is_none());
    assert!(value.get("parentPaneId").is_none());
}
