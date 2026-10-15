use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::routes::AppState;
use codeterm_service_core::multiplexer::types::Tab;

#[derive(Deserialize)]
pub struct WorkerObservationRequest {
    plan_id: String,
    task_id: String,
    attempt_id: String,
    worker_tab_id: String,
    recorded_nonce: Option<String>,
    presented_nonce: Option<String>,
    #[serde(default)]
    close: bool,
}

fn binding_matches(tab: &Tab, peer: &str, request: &WorkerObservationRequest) -> bool {
    tab.id == request.worker_tab_id
        && tab.report_target_mesh_peer.as_deref() == Some(peer)
        && tab.task_binding.as_ref().is_some_and(|binding| {
            binding.plan_id.as_deref() == Some(&request.plan_id)
                && binding.task_id.as_deref() == Some(&request.task_id)
                && binding.attempt_id.as_deref() == Some(&request.attempt_id)
        })
}

fn nonce_lineage_matches(tab: &Tab, recorded: Option<&str>, presented: Option<&str>) -> bool {
    tab.launch_session_authenticated()
        && recorded.is_some_and(|nonce| tab.binding_nonce_in_current_session_lineage(nonce))
        && presented.is_some_and(|nonce| tab.binding_nonce_in_current_session_lineage(nonce))
}

pub async fn mesh_worker_observation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WorkerObservationRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let peer_id = super::relay::check_mesh_relay_auth(&state, &headers)?;
    if !state
        .mesh
        .lock()
        .paired_peers
        .get(&peer_id)
        .is_some_and(|peer| super::peer_has_full_access(Some(&peer.access_level)))
    {
        return Err((
            StatusCode::FORBIDDEN,
            "worker observation requires Full mesh access".into(),
        ));
    }
    tokio::task::spawn_blocking(move || {
        let observation = {
            let mux = state.svc.mux.read();
            mux.get_tab(&request.worker_tab_id).map(|tab| {
                (
                    binding_matches(tab, &peer_id, &request),
                    nonce_lineage_matches(
                        tab,
                        request.recorded_nonce.as_deref(),
                        request.presented_nonce.as_deref(),
                    ),
                )
            })
        };
        match observation {
            None => Ok(Json(json!({"exists": false, "binding_matches": false}))),
            Some((false, _)) => Err((
                StatusCode::CONFLICT,
                "remote worker task binding changed".into(),
            )),
            Some((true, nonce_matches)) => {
                if request.close {
                    state
                        .svc
                        .close_tab(&request.worker_tab_id)
                        .map_err(|error| (StatusCode::CONFLICT, error))?;
                }
                Ok(Json(
                    json!({"exists": !request.close, "binding_matches": nonce_matches}),
                ))
            }
        }
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_service_core::multiplexer::state::MultiplexerState;

    #[test]
    fn remote_observation_requires_exact_peer_tab_and_task_attempt() {
        let (mut mux, id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.report_target_mesh_peer = Some("origin".into());
        tab.task_binding = Some(codeterm_service_core::multiplexer::types::TaskBinding {
            plan_id: Some("plan".into()),
            task_id: Some("task".into()),
            attempt_id: Some("attempt".into()),
            ..Default::default()
        });
        let mut request = WorkerObservationRequest {
            plan_id: "plan".into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            worker_tab_id: id,
            recorded_nonce: None,
            presented_nonce: None,
            close: false,
        };
        assert!(binding_matches(tab, "origin", &request));
        assert!(!binding_matches(tab, "foreign", &request));
        request.attempt_id = "other-attempt".into();
        assert!(!binding_matches(tab, "origin", &request));
        request.attempt_id = "attempt".into();
        request.worker_tab_id = "other-tab".into();
        assert!(!binding_matches(tab, "origin", &request));
    }

    #[test]
    fn remote_ack_accepts_only_same_session_nonce_lineage() {
        let (mut mux, id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("provider".into());
        tab.agent_session_id = Some("session".into());
        tab.session_binding_nonce = Some("launch".into());
        tab.remember_superseded_binding_nonce();
        tab.session_binding_nonce = Some("transition".into());
        tab.remember_superseded_binding_nonce();
        tab.session_binding_nonce = Some("current".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        assert!(nonce_lineage_matches(
            tab,
            Some("launch"),
            Some("transition")
        ));
        assert!(nonce_lineage_matches(tab, Some("launch"), Some("current")));
        assert!(!nonce_lineage_matches(
            tab,
            Some("foreign"),
            Some("current")
        ));
        assert!(!nonce_lineage_matches(tab, Some("launch"), Some("foreign")));
        assert!(!nonce_lineage_matches(tab, None, Some("current")));
        tab.agent_session_id = Some("different-session".into());
        assert!(!nonce_lineage_matches(tab, Some("launch"), Some("current")));
    }
}
