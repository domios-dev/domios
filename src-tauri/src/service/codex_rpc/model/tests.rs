use super::*;

#[test]
fn codex_rpc_settings_need_a_subsequent_own_turn_and_requested_effort() {
    let boundary = codeterm_types::agent::SessionModelTranscriptBoundary {
        last_entry_uuid: Some("current-turn".into()),
        last_entry_type: Some("rpc_turn".into()),
        ..Default::default()
    };
    let settings = json!({"model":"target", "effort":"high"});
    assert!(!cache_requested_settings(false, Some("high"), &settings));
    assert!(!cache_requested_settings(true, Some("low"), &settings));
    assert!(cache_requested_settings(true, Some("high"), &settings));
    assert!(!new_turn_settings(&boundary, None, Some("high"), &settings));
    assert!(!new_turn_settings(
        &boundary,
        Some("current-turn"),
        Some("high"),
        &settings
    ));
    assert!(!new_turn_settings(
        &boundary,
        Some("next-turn"),
        Some("low"),
        &settings
    ));
    assert!(!new_turn_settings(
        &boundary,
        Some("next-turn"),
        Some("high"),
        &json!({"model":"target"})
    ));
    assert!(new_turn_settings(
        &boundary,
        Some("next-turn"),
        Some("high"),
        &settings
    ));
    let idle = codeterm_types::agent::SessionModelTranscriptBoundary::default();
    assert!(!new_turn_settings(&idle, None, None, &settings));
    assert!(new_turn_settings(&idle, Some("next-turn"), None, &settings));
}

#[test]
fn codex_rpc_next_turn_request_uses_shared_pending_observation() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("own-thread".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    tab.agent_session_model = Some("observed".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let boundary = codeterm_types::agent::SessionModelTranscriptBoundary {
        last_entry_uuid: Some("prior-turn".into()),
        last_entry_type: Some("rpc_turn".into()),
        ..Default::default()
    };
    for (model, effort) in [("target", "medium"), ("target", "high"), ("back", "low")] {
        fixture
            .service
            .record_model_request(
                &tab_id,
                "fixture-provider",
                "own-thread",
                model,
                Some(effort),
                Some(boundary.clone()),
            )
            .unwrap();
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_session_model.as_deref(), Some("observed"));
        assert_eq!(
            tab.pending_model_switch_observation
                .as_ref()
                .unwrap()
                .launched_model,
            model
        );
        assert_eq!(
            tab.pending_model_switch_observation
                .as_ref()
                .unwrap()
                .transcript_boundary,
            boundary
        );
    }
}
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
            ..Default::default()
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
