use super::*;

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
