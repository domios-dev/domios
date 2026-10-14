use crate::plan::RemoteDispatchState;

pub(super) fn worker_authenticated_method(method: &str) -> bool {
    matches!(method, "plan.task.remote_ack" | "plan.task.remote_report")
}

pub(super) fn dispatch_error_state(error: &str) -> (RemoteDispatchState, &'static str) {
    if error.contains("peer_missing_repo") {
        (RemoteDispatchState::PeerMissingRepo, "peer_missing_repo")
    } else if error.contains("peer_missing_base") {
        (RemoteDispatchState::PeerMissingBase, "peer_missing_base")
    } else if error.starts_with("HTTP error:") {
        (RemoteDispatchState::PeerRefused, "peer_refused")
    } else {
        (RemoteDispatchState::RelayLost, "relay_lost")
    }
}

pub(super) fn dispatch_response(
    response: Result<serde_json::Value, String>,
    observed_spawn: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    response.or_else(|error| observed_spawn.ok_or(error))
}

#[cfg(test)]
mod tests {
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
            dispatch_response(
                Err("Network error: response lost".into()),
                Some(spawn.clone())
            ),
            Ok(spawn)
        );
        assert_eq!(
            dispatch_response(
                Ok(serde_json::json!({"worker_tab_id": "original"})),
                Some(serde_json::Value::Null)
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
            assert!(dispatch_response(Err(error.into()), None).is_err());
        }
        assert_eq!(
            dispatch_error_state("HTTP error: peer_worker_spawn_failed"),
            (RemoteDispatchState::PeerRefused, "peer_refused")
        );
        assert_eq!(
            dispatch_error_state("HTTP error: peer_missing_base"),
            (RemoteDispatchState::PeerMissingBase, "peer_missing_base")
        );
    }
}
