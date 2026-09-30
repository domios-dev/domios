use super::*;

#[test]
fn pending_model_switch_success_reports_ready_same_session_relaunch() {
    let session_id = "session-8b2c";
    let launched_model = "model-9e";
    let outcome = ModelSwitchOutcome {
        restart: crate::service::agent_restart::RestartOutcome {
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
        },
        mechanism: crate::agent::provider::ModelSwitchMode::RestartResume,
        previous_model: Some("model-old".into()),
        requested_model: launched_model.into(),
        launched_model: launched_model.into(),
        confirmation: ModelSwitchConfirmation::Pending,
    };

    let body = model_switch_success_body(&outcome);

    assert_eq!(body["ok"], true);
    assert_eq!(body["model_confirmation"], "pending");
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
