use super::*;

#[test]
fn staged_switch_exposes_pending_selection_without_restart_or_observation() {
    let body = model_switch_success_body(&ModelSwitchOutcome {
        tab_id: "tab".into(),
        provider: "provider".into(),
        session_id: "session".into(),
        restart: None,
        mechanism: crate::agent::provider::ModelSwitchMode::LiveCommand,
        fallback_reason: None,
        previous_model: None,
        requested_model: "selected".into(),
        launched_model: "selected".into(),
        confirmation_detail: None,
        confirmation: ModelSwitchConfirmation::AfterTurn,
    });
    assert_eq!(body["staged"], true);
    assert_eq!(body["requested_model"], "selected");
    assert_eq!(body["model_confirmation"], "pending");
    assert!(body.get("restart").is_none());
    assert!(body.get("observed_model").is_none());
}

#[test]
fn live_command_failure_offers_fallback_without_claiming_a_restart() {
    let error = ModelSwitchError::LiveCommandFailed("opaque failure".into());
    let body = model_switch_error_body(&error);
    assert_eq!(body["ok"], false);
    assert_eq!(body["reason"], "live_command_failed");
    assert_eq!(body["mode_used"], "live_command");
    assert_eq!(body["restart_fallback_available"], true);
    assert!(body.get("restart").is_none());
}

#[test]
fn live_command_success_preserves_session_and_has_no_restart() {
    let body = model_switch_success_body(&ModelSwitchOutcome {
        tab_id: "tab-opaque".into(),
        provider: "provider-opaque".into(),
        session_id: "session-opaque".into(),
        restart: None,
        mechanism: crate::agent::provider::ModelSwitchMode::LiveCommand,
        fallback_reason: None,
        previous_model: Some("previous-opaque".into()),
        requested_model: "chosen-opaque".into(),
        launched_model: "chosen-opaque".into(),
        confirmation_detail: None,
        confirmation: ModelSwitchConfirmation::Observed {
            model: "chosen-opaque".into(),
            evidence: ModelSwitchEvidence::LiveSessionRecord,
        },
    });
    assert_eq!(body["mode_used"], "live_command");
    assert_eq!(body["model_confirmation"], "observed");
    assert_eq!(body["session_id"], "session-opaque");
    assert!(body.get("restart").is_none());
}

#[test]
fn restart_guard_model_switch_refusal_names_running_items() {
    let items = vec!["shell-fixture".into(), "agent-fixture".into()];
    let error = ModelSwitchError::BackgroundWork {
        items: items.clone(),
    };
    assert_eq!(model_switch_error_status(&error), StatusCode::CONFLICT);
    let body = model_switch_error_body(&error);
    assert_eq!(body["reason"], "background_work_running");
    assert_eq!(body["running_items"], serde_json::json!(items));
}

#[test]
fn codex_rpc_control_model_request_validates_runtime_mode_and_effort() {
    let request: AgentModelBody = serde_json::from_value(serde_json::json!({
        "model":"selected", "reasoning_effort":"high", "runtime_mode":"auto", "force":true
    }))
    .unwrap();
    assert_eq!(request.model, "selected");
    assert!(request.force);
    assert_eq!(request.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(
        request.runtime_mode,
        Some(crate::service::agent_runtime::model::RuntimeMode::Auto)
    );
    assert!(serde_json::from_value::<AgentModelBody>(
        serde_json::json!({"model":"selected","runtime_mode":"unknown"})
    )
    .is_err());
}

#[test]
fn codex_rpc_control_next_turn_response_pending_then_observed_has_no_restart() {
    let outcome = ModelSwitchOutcome {
        tab_id: "bound-tab".into(),
        provider: "declared-provider".into(),
        session_id: "bound-thread".into(),
        restart: None,
        mechanism: crate::agent::provider::ModelSwitchMode::NextTurn,
        fallback_reason: None,
        previous_model: Some("previous".into()),
        requested_model: "selected".into(),
        launched_model: "selected".into(),
        confirmation_detail: None,
        confirmation: ModelSwitchConfirmation::Pending,
    };
    let pending = model_switch_success_body(&outcome);
    assert_eq!(pending["mode_used"], "next_turn");
    assert_eq!(pending["model_confirmation"], "pending");
    assert!(pending.get("restart").is_none());
    assert!(pending.get("observed_model").is_none());
    let observed = model_switch_success_body(&ModelSwitchOutcome {
        confirmation: ModelSwitchConfirmation::Observed {
            model: "selected".into(),
            evidence: ModelSwitchEvidence::LiveSessionRecord,
        },
        ..outcome
    });
    assert_eq!(observed["model_confirmation"], "observed");
    assert_eq!(observed["observed_model"], "selected");
    assert!(observed.get("restart").is_none());
}

#[test]
fn pending_model_switch_success_reports_ready_same_session_relaunch() {
    let session_id = "session-8b2c";
    let launched_model = "model-9e";
    let outcome = ModelSwitchOutcome {
        tab_id: "tab-4f3a".into(),
        provider: "provider-7d1a".into(),
        session_id: session_id.into(),
        restart: Some(crate::service::agent_restart::RestartOutcome {
            tab_id: "tab-4f3a".into(),
            provider: "provider-7d1a".into(),
            session_id: Some(session_id.into()),
            session_was_stale: false,
            force: false,
            auto_approve: false,
            restart_accepted: true,
            old_pty_torn_down: true,
            agent_ready: true,
            binding_restored: true,
            recovered_in_place: false,
            old_pty_id: Some("pty-old-3a".into()),
            new_pty_id: Some("pty-new-3a".into()),
        }),
        mechanism: crate::agent::provider::ModelSwitchMode::RestartResume,
        fallback_reason: None,
        previous_model: Some("model-old".into()),
        requested_model: launched_model.into(),
        launched_model: launched_model.into(),
        confirmation_detail: None,
        confirmation: ModelSwitchConfirmation::Pending,
    };

    let body = model_switch_success_body(&outcome);

    assert_eq!(body["ok"], true);
    assert_eq!(body["model_confirmation"], "pending");
    assert_eq!(body["mode_used"], "restart_resume");
    assert_eq!(body["session_id"], session_id);
    assert_eq!(body["launched_model"], launched_model);
    assert_eq!(body["relaunch_evidence"]["session_id"], session_id);
    assert_eq!(body["relaunch_evidence"]["launched_model"], launched_model);
    assert_eq!(body["relaunch_evidence"]["agent_ready"], true);
    assert_eq!(body["restart"]["agent_ready"], true);
    assert!(body.get("observed_model").is_none());

    let observed = model_switch_success_body(&ModelSwitchOutcome {
        confirmation: ModelSwitchConfirmation::Observed {
            model: launched_model.into(),
            evidence: ModelSwitchEvidence::FirstPostRelaunchTurn,
        },
        ..outcome
    });
    assert_eq!(observed["ok"], true);
    assert_eq!(observed["model_confirmation"], "observed");
    assert_eq!(observed["observed_model"], launched_model);
    assert_eq!(observed["evidence"], "first_post_relaunch_turn");
}

#[test]
fn live_model_switch_success_reports_the_bound_session_record() {
    let body = model_switch_success_body(&ModelSwitchOutcome {
        tab_id: "tab-live".into(),
        provider: "opencode".into(),
        session_id: "ses-live".into(),
        restart: None,
        mechanism: crate::agent::provider::ModelSwitchMode::Live,
        fallback_reason: None,
        previous_model: Some("provider/old".into()),
        requested_model: "provider/new".into(),
        launched_model: "provider/new".into(),
        confirmation_detail: None,
        confirmation: ModelSwitchConfirmation::Observed {
            model: "provider/new".into(),
            evidence: ModelSwitchEvidence::LiveSessionRecord,
        },
    });

    assert_eq!(body["mode_used"], "live");
    assert_eq!(body["model_confirmation"], "observed");
    assert_eq!(body["evidence"], "live_session_record");
    assert_eq!(body["session_id"], "ses-live");
    assert!(body.get("restart").is_none());
}

#[test]
fn runtime_control_refusal_is_typed_and_never_offers_restart_fallback() {
    let error = ModelSwitchError::RuntimeControlRefused("provider refused settings".into());
    assert_eq!(model_switch_error_status(&error), StatusCode::CONFLICT);
    let body = model_switch_error_body(&error);
    assert_eq!(body["reason"], "runtime_control_refused");
    assert_eq!(body["mode_used"], "live");
    assert_eq!(body["ok"], false);
    assert!(body.get("restart_fallback_available").is_none());
}
