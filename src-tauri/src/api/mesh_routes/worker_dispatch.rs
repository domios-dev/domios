use axum::{extract::State as AxumState, http::StatusCode, Json};
use serde::Deserialize;

use crate::api::routes::AppState;

#[derive(Deserialize)]
pub struct MeshWorkerDispatchRequest {
    repository_identity: String,
    base_commit: String,
    plan_id: String,
    task_id: String,
    attempt_id: String,
    origin_tab_id: String,
    provider: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
    brief: String,
}

pub async fn mesh_worker_dispatch(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<MeshWorkerDispatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let origin_peer_id = super::relay::check_mesh_relay_auth(&state, &headers)?;
    tokio::task::spawn_blocking(move || dispatch_authenticated_worker(state, origin_peer_id, request))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

fn dispatch_authenticated_worker(
    state: AppState,
    origin_peer_id: String,
    request: MeshWorkerDispatchRequest,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let origin_peer_name = {
        let mesh = state.mesh.lock();
        let Some(peer) = mesh.paired_peers.get(&origin_peer_id) else {
            return Err((StatusCode::UNAUTHORIZED, "origin peer is not paired".into()));
        };
        if peer.access_level != crate::mesh::types::AccessLevel::Full {
            return Err((
                StatusCode::FORBIDDEN,
                "remote worker dispatch requires Full mesh access".into(),
            ));
        }
        peer.instance_name.clone()
    };
    let workspaces = {
        let mux = state.svc.mux.read();
        mux.list_workspaces()
            .into_iter()
            .filter_map(|workspace| workspace.cwd.map(|cwd| (workspace.id, cwd)))
            .collect::<Vec<_>>()
    };
    let matched = workspaces.into_iter().find(|(_, cwd)| {
        !crate::service::worker_lane::is_linked_worktree(std::path::Path::new(cwd))
            && crate::service::worker_lane::repository_identity(cwd)
                .is_ok_and(|identity| identity == request.repository_identity)
    });
    let Some((workspace_id, canonical_cwd)) = matched else {
        return Err((
            StatusCode::CONFLICT,
            "peer_missing_repo: no primary checkout has the requested repository identity".into(),
        ));
    };
    if !crate::service::worker_lane::contains_commit(&canonical_cwd, &request.base_commit) {
        return Err((
            StatusCode::CONFLICT,
            "peer_missing_base: requested base commit is absent from the peer checkout".into(),
        ));
    }
    let lane_label = format!("{}-{}", request.plan_id, request.task_id);
    let lane = crate::service::worker_lane::provision_isolated_lane_at_base(
        &canonical_cwd,
        &lane_label,
        &request.base_commit,
    )
    .map_err(|error| (StatusCode::CONFLICT, format!("peer_lane_provision_failed: {error}")))?;
    let provenance = match lane.verify() {
        Ok(provenance) => provenance,
        Err(error) => {
            let cleanup = lane.cleanup();
            return Err((
                StatusCode::CONFLICT,
                match cleanup {
                    Ok(()) => format!("peer_lane_verification_failed: {error}"),
                    Err(cleanup) => format!("peer_lane_verification_failed: {error}; cleanup failed: {cleanup}"),
                },
            ));
        }
    };
    let suffix: String = request.attempt_id.chars().take(6).collect();
    let spawn = crate::service::SpawnAgentParams {
        agent: Some(request.provider),
        cwd: Some(provenance.worktree_path.clone()),
        provisioned_cwd: Some(provenance.worktree_path.clone()),
        title: Some(format!("Remote{suffix}")),
        description: Some(format!("Plan task {} on origin peer {}", request.task_id, origin_peer_name)),
        role: Some("Worker".into()),
        workspace_id: Some(workspace_id),
        task: Some(request.brief),
        report_target_tab_id: Some(request.origin_tab_id),
        report_target_mesh_peer: Some(origin_peer_name),
        model: request.model,
        reasoning_effort: request.reasoning_effort,
        ..Default::default()
    };
    let result = match state.svc.spawn_agent_sync(spawn) {
        Ok(result) => result,
        Err(error) => {
            let cleanup = lane.cleanup();
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                match cleanup {
                    Ok(()) => format!("peer_worker_spawn_failed: {error}"),
                    Err(cleanup) => format!("peer_worker_spawn_failed: {error}; lane cleanup failed: {cleanup}"),
                },
            ));
        }
    };
    Ok(Json(serde_json::json!({
        "state": "awaiting_worker_ack",
        "peer_id": state.mesh.lock().ident.identity.instance_id,
        "worker_tab_id": result.tab_id,
        "title": result.title,
        "branch": provenance.branch,
        "worktree_path": provenance.worktree_path,
        "base_commit": provenance.base,
        "repository_identity": request.repository_identity,
    })))
}
