use super::*;

#[test]
fn restart_response_reports_observed_transition_legs() {
    let readiness = crate::service::boot_stage::classify_restart_readiness(
        Some("pty-new"),
        Some(codeterm_types::pty::BootStage::AgentReady),
        None,
        Some("pty-old"),
    );
    assert_eq!(
        readiness,
        Some(crate::service::boot_stage::RestartReadiness::Ready {
            pty_id: "pty-new".into(),
        })
    );

    let outcome = crate::service::agent_restart::RestartOutcome {
        tab_id: "tab-1".into(),
        provider: "provider-fixture".into(),
        session_id: Some("session-1".into()),
        session_was_stale: false,
        force: false,
        auto_approve: true,
        restart_accepted: true,
        old_pty_torn_down: true,
        agent_ready: true,
        binding_restored: true,
        recovered_in_place: false,
        old_pty_id: Some("pty-old".into()),
        new_pty_id: Some("pty-new".into()),
    };

    let body = agent_restart_response(&outcome);

    assert_eq!(body["restart_accepted"], true);
    assert_eq!(body["old_pty_torn_down"], true);
    assert_eq!(body["agent_ready"], true);
    assert_eq!(body["recovered_in_place"], false);
    assert_eq!(body["old_pty_id"], "pty-old");
    assert_eq!(body["new_pty_id"], "pty-new");
    assert_eq!(body["full_access"], true);
    assert!(body.get("permission_profile").is_none());
    assert!(body.get("auto_approve").is_none());
    assert!(body.get("restarting").is_none());
}

#[test]
fn exit_recovery_response_reports_the_same_pty_resumed_in_place() {
    let outcome = crate::service::agent_restart::RestartOutcome {
        tab_id: "tab-1".into(),
        provider: "provider-fixture".into(),
        session_id: Some("session-1".into()),
        session_was_stale: false,
        force: false,
        auto_approve: false,
        restart_accepted: true,
        old_pty_torn_down: false,
        agent_ready: true,
        binding_restored: true,
        recovered_in_place: true,
        old_pty_id: Some("pty-shell".into()),
        new_pty_id: Some("pty-shell".into()),
    };

    let body = agent_restart_response(&outcome);

    assert_eq!(body["recovered_in_place"], true);
    assert_eq!(body["old_pty_torn_down"], false);
    assert_eq!(body["session_action"], "resume");
    assert_eq!(body["session_id"], "session-1");
    assert_eq!(body["old_pty_id"], body["new_pty_id"]);
}

#[test]
fn stale_pre_restart_agent_ready_reports_readiness_was_not_observed() {
    let readiness = crate::service::boot_stage::classify_restart_readiness(
        Some("pty-old"),
        Some(codeterm_types::pty::BootStage::AgentReady),
        None,
        Some("pty-old"),
    );
    assert_eq!(readiness, None);

    let outcome = crate::service::agent_restart::RestartOutcome {
        tab_id: "tab-1".into(),
        provider: "provider-fixture".into(),
        session_id: Some("session-1".into()),
        session_was_stale: false,
        force: false,
        auto_approve: true,
        restart_accepted: true,
        old_pty_torn_down: true,
        agent_ready: false,
        binding_restored: false,
        recovered_in_place: false,
        old_pty_id: Some("pty-old".into()),
        new_pty_id: None,
    };

    let body = agent_restart_failure_response(
        &outcome,
        "restart accepted but new agent readiness was not observed",
    );

    assert_eq!(body["restart_accepted"], true);
    assert_eq!(body["old_pty_torn_down"], true);
    assert_eq!(body["agent_ready"], false);
    assert_eq!(body["new_pty_id"], serde_json::Value::Null);
    assert!(body["error"]
        .as_str()
        .is_some_and(|error| error.contains("readiness was not observed")));
}
