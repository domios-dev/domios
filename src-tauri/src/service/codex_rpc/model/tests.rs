use super::*;
#[test]
fn codex_rpc_control_runtime_modes_and_next_turn_overrides() {
    for (mode, policy, reviewer, sandbox) in [
        (
            RuntimeMode::ApprovalRequired,
            "untrusted",
            "user",
            "readOnly",
        ),
        (
            RuntimeMode::AutoAcceptEdits,
            "on-request",
            "user",
            "workspaceWrite",
        ),
        (
            RuntimeMode::Auto,
            "on-request",
            "auto_review",
            "workspaceWrite",
        ),
        (RuntimeMode::FullAccess, "never", "user", "dangerFullAccess"),
    ] {
        let controls = TurnControls {
            model: Some("selected".into()),
            effort: Some("high".into()),
            mode,
        };
        let value = controls.overrides();
        assert_eq!(value["model"], "selected");
        assert_eq!(value["effort"], "high");
        assert_eq!(value["approvalPolicy"], policy);
        assert_eq!(value["approvalsReviewer"], reviewer);
        assert_eq!(value["sandboxPolicy"]["type"], sandbox);
    }
}
#[test]
fn codex_rpc_recorded_settings_reconfirm_restored_effort_for_current_binding() {
    use crate::service::reasoning_effort::{
        apply_session_owned_actual, clear_runtime_evidence, persist_launch_identity,
    };
    let events = include_str!(
        "../../../../../evals/codex-remote-spike/evidence/windows-inject-observer.ndjson"
    );
    let frame = events
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|entry| {
            entry["kind"] == "receive" && entry["value"]["method"] == "thread/settings/updated"
        })
        .expect("recorded settings notification");
    let frame = super::super::protocol::decode(&frame["value"].to_string()).unwrap();
    let super::super::protocol::Frame::Notification { params, .. } = frame else {
        panic!("notification required")
    };
    let thread = params["threadId"].as_str().unwrap();
    let actual = session_effort(&params["threadSettings"]).unwrap();
    assert_eq!(actual.as_deref(), Some("medium"));
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some(thread.into());
    tab.session_binding_nonce = Some("restored-binding".into());
    tab.reported_session_binding_nonce = Some("restored-binding".into());
    persist_launch_identity(tab, None, Some("low"));
    tab.reasoning_effort_identity.actual = Some("low".into());
    clear_runtime_evidence(tab);
    assert_eq!(tab.reasoning_effort_identity.actual, None);
    assert_eq!(
        tab.reasoning_effort_identity.actual_source.as_deref(),
        Some("provider-default-unverified")
    );
    assert!(!apply_session_owned_actual(
        tab,
        "fixture-provider",
        "foreign-session",
        Some("restored-binding"),
        actual.clone()
    ));
    assert!(!apply_session_owned_actual(
        tab,
        "fixture-provider",
        thread,
        Some("stale-binding"),
        actual.clone()
    ));
    assert!(apply_session_owned_actual(
        tab,
        "fixture-provider",
        thread,
        Some("restored-binding"),
        actual.clone()
    ));
    assert_eq!(
        tab.reasoning_effort_identity.actual.as_deref(),
        Some("medium")
    );
    assert_eq!(
        tab.reasoning_effort_identity.actual_source.as_deref(),
        Some("session-owned")
    );
    assert_eq!(
        tab.reasoning_effort_identity.requested.as_deref(),
        Some("low")
    );
    assert_eq!(
        tab.reasoning_effort_identity.resolved.as_deref(),
        Some("low")
    );
    assert!(!apply_session_owned_actual(
        tab,
        "fixture-provider",
        thread,
        Some("restored-binding"),
        actual
    ));
}

#[test]
fn codex_rpc_effort_settings_distinguish_absent_unknown_and_unset() {
    assert_eq!(session_effort(&json!({"model":"m"})), None);
    assert_eq!(session_effort(&json!({"effort":42})), None);
    assert_eq!(session_effort(&json!({"effort":" "})), None);
    assert_eq!(session_effort(&json!({"effort":null})), Some(None));
    assert_eq!(
        session_effort(&json!({"effort":" low "})),
        Some(Some("low".into()))
    );
}
