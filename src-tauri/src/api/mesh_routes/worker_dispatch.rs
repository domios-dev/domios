use axum::{extract::State as AxumState, http::StatusCode, Json};
use serde::Deserialize;
use serde::Serialize;

use crate::api::routes::AppState;

fn peer_has_full_access(access: Option<&crate::mesh::types::AccessLevel>) -> bool {
    access == Some(&crate::mesh::types::AccessLevel::Full)
}

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

#[derive(Deserialize)]
pub struct MeshWorkerLaneBundleRequest {
    repository_identity: String,
    branch: String,
    base_commit: String,
    plan_id: String,
    task_id: String,
    attempt_id: String,
    report_sha: String,
}

pub async fn mesh_worker_lane_bundle(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<MeshWorkerLaneBundleRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let peer_id = super::relay::check_mesh_relay_auth(&state, &headers)?;
    tokio::task::spawn_blocking(move || create_lane_bundle(state, peer_id, request))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

fn create_lane_bundle(
    state: AppState,
    peer_id: String,
    request: MeshWorkerLaneBundleRequest,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let allowed = {
        let mesh = state.mesh.lock();
        mesh.paired_peers.get(&peer_id)
            .is_some_and(|peer| peer_has_full_access(Some(&peer.access_level)))
    };
    if !allowed {
        return Err((StatusCode::FORBIDDEN, "lane bundle requires Full mesh access".into()));
    }
    let lane_cwd = {
        let mux = state.svc.mux.read();
        mux.all_tabs().find_map(|tab| {
            let binding = tab.task_binding.as_ref()?;
            (binding.plan_id.as_deref() == Some(&request.plan_id)
                && binding.task_id.as_deref() == Some(&request.task_id)
                && binding.attempt_id.as_deref() == Some(&request.attempt_id)
                && tab.report_target_mesh_peer.as_deref() == Some(peer_id.as_str()))
                .then(|| tab.provisioned_cwd.clone().or_else(|| tab.cwd.clone()))?
        })
    };
    let checkout = lane_cwd.ok_or_else(|| (StatusCode::NOT_FOUND, "remote attempt lane not found for authenticated origin".into()))?;
    if !crate::service::worker_lane::repository_identity(&checkout)
        .is_ok_and(|identity| identity == request.repository_identity.as_str())
    {
        return Err((StatusCode::CONFLICT, "remote attempt repository identity mismatch".into()));
    }
    if !crate::service::worker_lane::contains_commit(&checkout, &request.base_commit) {
        return Err((StatusCode::CONFLICT, "peer_missing_base: base commit is absent".into()));
    }
    let reference = format!("refs/heads/{}", request.branch);
    let mut command = std::process::Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let output = command.current_dir(&checkout).args(["check-ref-format", &reference]).output()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if !output.status.success() || !request.branch.starts_with("lane/") {
        return Err((StatusCode::BAD_REQUEST, "invalid peer lane branch".into()));
    }
    if !crate::service::worker_lane::current_branch(&checkout)
        .is_ok_and(|branch| branch == request.branch.as_str())
    {
        return Err((StatusCode::CONFLICT, "attempt checkout is not on the requested lane branch".into()));
    }
    let lane_tip = crate::service::worker_lane::git_value(&checkout, ["rev-parse", &reference])
        .map_err(|error| (StatusCode::CONFLICT, error))?;
    if lane_tip != request.report_sha {
        return Err((StatusCode::CONFLICT, "peer lane tip differs from accepted report SHA".into()));
    }
    if !crate::service::worker_lane::git_value(&checkout, ["rev-parse", "HEAD"])
        .is_ok_and(|head| head == request.report_sha.as_str()) {
        return Err((StatusCode::CONFLICT, "attempt checkout HEAD differs from accepted report SHA".into()));
    }
    if !crate::service::worker_lane::is_ancestor(&checkout, &request.base_commit, &lane_tip) {
        return Err((StatusCode::CONFLICT, "peer lane does not descend from requested base".into()));
    }
    let temp = tempfile::tempdir()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let bundle_path = temp.path().join("worker-lane.bundle");
    let mut command = std::process::Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let output = command.current_dir(&checkout).args(["bundle", "create"])
        .arg(&bundle_path).arg(&reference).output()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::CONFLICT, String::from_utf8_lossy(&output.stderr).trim().to_string()));
    }
    const MAX_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;
    let size = std::fs::metadata(&bundle_path).map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?.len();
    if size > MAX_BUNDLE_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "worker lane bundle exceeds 32 MiB".into()));
    }
    let bytes = std::fs::read(&bundle_path).map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    use base64::Engine;
    #[derive(Serialize)]
    struct BundleResponse { branch: String, base_commit: String, lane_tip: String, bundle: String }
    Ok(Json(serde_json::to_value(BundleResponse {
        branch: request.branch,
        base_commit: request.base_commit,
        lane_tip,
        bundle: base64::engine::general_purpose::STANDARD.encode(bytes),
    }).unwrap()))
}

#[cfg(test)]
mod tests {
    use super::peer_has_full_access;
    use crate::mesh::types::AccessLevel;

    #[test]
    fn worker_dispatch_rejects_unpaired_and_non_full_peers() {
        assert!(!peer_has_full_access(None));
        assert!(!peer_has_full_access(Some(&AccessLevel::ReadOnly)));
        assert!(peer_has_full_access(Some(&AccessLevel::Full)));
    }
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
    let (origin_peer_name, origin_peer_id) = {
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
        (peer.instance_name.clone(), origin_peer_id)
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
                .is_ok_and(|identity| identity == request.repository_identity.as_str())
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
        plan_id: Some(request.plan_id.clone()),
        task_id: Some(request.task_id.clone()),
        task_launch_authority: Some(crate::service::TaskLaunchAuthority {
            plan_id: request.plan_id.clone(),
            task_id: request.task_id.clone(),
            attempt_id: request.attempt_id.clone(),
            lane_base: Some(request.base_commit.clone()),
        }),
        execution_intent: Some(crate::plan::ExecutionIntent::Mutating),
        workspace_id: Some(workspace_id),
        task: Some(request.brief),
        report_target_tab_id: Some(request.origin_tab_id),
        report_target_mesh_peer: Some(origin_peer_id.clone()),
        remote_task_binding: Some(crate::service::RemoteTaskBinding {
            plan_id: request.plan_id.clone(),
            task_id: request.task_id.clone(),
            attempt_id: request.attempt_id.clone(),
            origin_peer_id,
        }),
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
