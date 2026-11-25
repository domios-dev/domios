use super::*;

#[test]
fn only_remote_worker_lifecycle_methods_bypass_owner_guard() {
    for method in ["plan.task.remote_ack", "plan.task.remote_report"] {
        assert!(worker_authenticated_method(method));
    }
    for method in [
        "plan.task.update",
        "plan.task.dispatch",
        "plan.task.close",
        "plan.task.assign",
        "plan.task.merge_remote_lane",
        "task.take",
        "task.drop",
        "plan.task.remote_unknown",
    ] {
        assert!(!worker_authenticated_method(method));
    }
}

#[test]
fn observed_spawn_wins_over_lost_dispatch_response() {
    let spawn = serde_json::json!({"worker_tab_id": "worker", "state": "awaiting_worker_ack"});
    assert_eq!(
        dispatch_response(Err("Network error: response lost".into()), || Ok(
            spawn.clone()
        )),
        Ok(spawn)
    );
    assert_eq!(
        dispatch_response(
            Ok(serde_json::json!({"worker_tab_id": "original"})),
            || panic!("successful dispatch must not probe status")
        ),
        Ok(serde_json::json!({"worker_tab_id": "original"}))
    );
}

#[test]
fn response_loss_does_not_prove_peer_offline() {
    for error in [
        "Network error: error sending request",
        "unroutable",
        "Bad response: expected value",
    ] {
        assert_eq!(
            dispatch_error_state(error),
            (RemoteDispatchState::RelayLost, "relay_lost")
        );
        assert!(dispatch_response(Err(error.into()), || Err("not observed".into())).is_err());
    }
    assert_eq!(
        dispatch_error_state("HTTP error: peer_worker_spawn_failed"),
        (RemoteDispatchState::PeerRefused, "peer_refused")
    );
    assert_eq!(
        dispatch_error_state(
            "HTTP error: 409 Conflict: peer_provider_auth_required: Codex ('codex') is not signed in on peer 'ROG'"
        ),
        (
            RemoteDispatchState::PeerRefused,
            "peer_provider_auth_required"
        )
    );
    assert_eq!(
        dispatch_error_state("HTTP error: peer_missing_base"),
        (RemoteDispatchState::PeerMissingBase, "peer_missing_base")
    );
}

#[test]
fn pending_status_after_response_loss_preserves_uncertain_attempt() {
    let pending = "HTTP error: 409 Conflict: peer_worker_launch_pending: launch in progress";
    let error = dispatch_response(Err("Network error: response lost".into()), || {
        Err(pending.into())
    })
    .unwrap_err();
    assert_eq!(error, pending);
    assert_eq!(
        dispatch_error_state(&error),
        (RemoteDispatchState::RelayLost, "relay_lost")
    );
    let refused = "HTTP error: 409 Conflict: peer_provider_auth_required";
    assert_eq!(
        dispatch_response(Err(refused.into()), || panic!(
            "definitive refusal must not recover a tab"
        )),
        Err(refused.into())
    );
}
