use codeterm_service_core::plan::RemoteDispatchState;

pub(super) fn worker_authenticated_method(method: &str) -> bool {
    matches!(method, "plan.task.remote_ack" | "plan.task.remote_report")
}

pub(super) fn dispatch_error_state(error: &str) -> (RemoteDispatchState, &'static str) {
    if error.contains("peer_worker_launch_pending") {
        (RemoteDispatchState::RelayLost, "relay_lost")
    } else if error.contains("peer_missing_repo") {
        (RemoteDispatchState::PeerMissingRepo, "peer_missing_repo")
    } else if error.contains("peer_missing_base") {
        (RemoteDispatchState::PeerMissingBase, "peer_missing_base")
    } else if error.contains("peer_provider_auth_required") {
        (
            RemoteDispatchState::PeerRefused,
            "peer_provider_auth_required",
        )
    } else if error.starts_with("HTTP error:") {
        (RemoteDispatchState::PeerRefused, "peer_refused")
    } else {
        (RemoteDispatchState::RelayLost, "relay_lost")
    }
}

pub(super) fn dispatch_response(
    response: Result<serde_json::Value, String>,
    observe: impl FnOnce() -> Result<serde_json::Value, String>,
) -> Result<serde_json::Value, String> {
    response.or_else(|error| {
        if error.starts_with("HTTP error:") {
            return Err(error);
        }
        match observe() {
            Ok(spawn) => Ok(spawn),
            Err(status) if status.contains("peer_worker_launch_pending") => Err(status),
            Err(_) => Err(error),
        }
    })
}

#[cfg(test)]
mod tests;
