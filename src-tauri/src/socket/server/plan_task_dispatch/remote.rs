use serde_json::{json, Value};
use tauri::Manager;

use crate::plan::RemoteDispatchState;
use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::{dispatch_authorized_gates, invalid_dispatch, owned_str};
use crate::socket::server::{plan_refs, JsonRpcError};

fn encoded_bundle_size_allowed(encoded_size: usize, max_bundle_bytes: usize) -> bool {
    encoded_size <= max_bundle_bytes.div_ceil(3) * 4
}

pub(super) fn dispatch(
    params: &Option<Value>,
    app: &tauri::AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let peer_target = owned_str(p, "mesh_peer")
        .filter(|peer| !peer.trim().is_empty())
        .ok_or_else(|| invalid_dispatch("mesh dispatch requires a paired peer instance ID"))?;
    let (plan_id, task_id) = plan_refs::resolve_task(&svc.plan_store, params)?;
    let detail = svc
        .plan_store
        .show_task(&task_id)
        .ok_or_else(|| super::super::plan::plan_rpc_err(PlanError::TaskNotFound))?;
    super::preflight::preflight_dispatch_task_status(&plan_id, &detail.task)?;
    if owned_str(p, "execution_intent").as_deref() != Some("mutating")
        || !detail.task.execution_intent.requires_repository()
    {
        return Err(invalid_dispatch(
            "mesh dispatch requires a repository task with mutating execution intent",
        ));
    }
    let caller = crate::service::AuthenticatedPlanCaller {
        tab_id: owned_str(p, "caller_tab_id"),
        session_id: owned_str(p, "session_id"),
        binding_nonce: owned_str(p, "session_binding_nonce"),
    };
    svc.authorize_execution_action(&plan_id, &caller)
        .map_err(|message| JsonRpcError {
            code: -32003,
            message,
            data: Some(json!({"type": "plan_orchestrator_authorization_required"})),
        })?;
    let plan = svc
        .plan_store
        .get(&plan_id)
        .ok_or_else(|| super::super::plan::plan_rpc_err(PlanError::NotFound))?;
    let canonical_cwd = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.clone()
    } else {
        plan.canonical_project_path.clone()
    };
    crate::service::worker_lane::verify_worker_lane(
        &canonical_cwd,
        &canonical_cwd,
        None,
        None,
        crate::service::LaneProtection::Enforced,
    )
    .map_err(|error| invalid_dispatch(&error))?;
    let base = git_head(&canonical_cwd).map_err(|error| invalid_dispatch(&error))?;
    let destination = crate::service::worker_lane::current_branch(&canonical_cwd)
        .map_err(|error| invalid_dispatch(&error))?;
    if destination.is_empty() {
        return Err(invalid_dispatch(
            "mesh dispatch requires an attached destination branch",
        ));
    }
    if detail
        .task
        .lane
        .destination
        .as_deref()
        .is_some_and(|requested| !requested.trim().is_empty() && requested != destination.as_str())
    {
        return Err(invalid_dispatch(
            "canonical checkout is not on the task's requested destination branch",
        ));
    }
    let repository_identity = crate::service::worker_lane::repository_identity(&canonical_cwd)
        .map_err(|error| invalid_dispatch(&error))?;
    let authorized_gates = dispatch_authorized_gates(p)?;
    let requested_model = owned_str(p, "model").or_else(|| detail.task.model.clone());
    let model = crate::service::spawn::resolve_agent_model_selection(
        &svc.agent_registry,
        owned_str(p, "agent").as_deref(),
        requested_model.as_deref(),
        &crate::config::default_agent_provider(),
    )
    .map_err(super::super::model_errors::invalid_model_rpc_error)?;
    super::preflight::preflight_review_capacity(&plan, &detail.task)
        .map_err(super::super::plan::plan_rpc_err)?;
    match super::regression_dispatch_decision(&plan, &detail.task) {
        super::RegressionDispatchDecision::Clear
        | super::RegressionDispatchDecision::Exempt { .. } => {}
        super::RegressionDispatchDecision::Blocked {
            blocking,
            retrospectives,
        } => {
            return Err(super::regression_blocked_error(blocking, retrospectives));
        }
    }
    if p.and_then(|value| value.get("placement"))
        .and_then(Value::as_str)
        .is_some_and(|placement| placement == "canonical_writer")
    {
        return Err(invalid_dispatch("mesh Workers always require a peer lane"));
    }
    let attempt_id = crate::plan::ids::short_id("attempt");
    let (peer_id, peer_name, peer_client, peer_state) = select_peer(app, &peer_target)?;
    svc.plan_store
        .begin_remote_dispatch_attempt(
            &plan_id,
            &task_id,
            &attempt_id,
            &peer_id,
            &peer_name,
            &model.provider_id,
            &base,
            &destination,
            now,
        )
        .map_err(super::super::plan::plan_rpc_err)?;
    if peer_state == PeerState::Offline || peer_client.is_none() {
        svc.plan_store
            .set_remote_dispatch_state(
                &plan_id,
                &task_id,
                &attempt_id,
                &peer_id,
                RemoteDispatchState::PeerOffline,
                now,
            )
            .map_err(super::super::plan::plan_rpc_err)?;
        super::emit_plan_changed(app, &plan_id);
        return Err(remote_failure(
            "peer_offline",
            &attempt_id,
            "paired peer is offline; the origin attempt is retained for retry",
        ));
    }
    let manifest =
        crate::service::plan::plan_skills::plan_skill_manifest(&svc.plan_store, &plan_id)
            .map_err(super::super::plan::plan_rpc_err)?;
    let brief = super::brief::build_brief_with_manifest(
        &detail.task,
        &detail.sections,
        &plan_id,
        &manifest,
        &authorized_gates,
    );
    let origin_id = app
        .state::<crate::MeshStateHandle>()
        .lock()
        .ident
        .identity
        .instance_id
        .clone();
    let brief = remote_worker_brief(brief, &plan_id, &task_id, &attempt_id, &origin_id);
    let origin_tab_id = owned_str(p, "caller_tab_id")
        .or(plan.owner.tab_id.clone())
        .ok_or_else(|| invalid_dispatch("mesh dispatch requires the origin Orchestrator tab"))?;
    let request = json!({
        "repository_identity": repository_identity,
        "base_commit": base,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "origin_tab_id": origin_tab_id,
        "provider": model.provider_id,
        "model": model.model,
        "reasoning_effort": owned_str(p, "reasoning_effort"),
        "brief": brief,
    });
    let Some(client) = peer_client else {
        return Err(remote_failure(
            "peer_offline",
            &attempt_id,
            "paired peer has no reachable signed route",
        ));
    };
    let response = tauri::async_runtime::block_on(
        client.auth_post_json("/api/mesh/worker-dispatch", &request),
    );
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let (state, label) = remote_error_state(&error);
            svc.plan_store
                .set_remote_dispatch_state(&plan_id, &task_id, &attempt_id, &peer_id, state, now)
                .map_err(super::super::plan::plan_rpc_err)?;
            super::emit_plan_changed(app, &plan_id);
            return Err(remote_failure(label, &attempt_id, &error));
        }
    };
    let worker_tab_id = response.get("worker_tab_id").and_then(Value::as_str);
    let Some(worker_tab_id) = worker_tab_id else {
        svc.plan_store
            .set_remote_dispatch_state(
                &plan_id,
                &task_id,
                &attempt_id,
                &peer_id,
                RemoteDispatchState::RelayLost,
                now,
            )
            .map_err(super::super::plan::plan_rpc_err)?;
        super::emit_plan_changed(app, &plan_id);
        return Err(remote_failure(
            "relay_lost",
            &attempt_id,
            "peer response omitted worker_tab_id; the origin attempt is retained",
        ));
    };
    let branch = response
        .get("branch")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let worktree_path = response
        .get("worktree_path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let lane_base = response
        .get("base_commit")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if branch.is_empty() || worktree_path.is_empty() || lane_base != base {
        svc.plan_store
            .set_remote_dispatch_state(
                &plan_id,
                &task_id,
                &attempt_id,
                &peer_id,
                RemoteDispatchState::RelayLost,
                now,
            )
            .map_err(super::super::plan::plan_rpc_err)?;
        super::emit_plan_changed(app, &plan_id);
        return Err(remote_failure(
            "relay_lost",
            &attempt_id,
            "peer response did not contain verified lane references",
        ));
    }
    svc.plan_store
        .record_remote_dispatch_lane(
            &plan_id,
            &task_id,
            &attempt_id,
            &peer_id,
            worker_tab_id,
            branch,
            worktree_path,
            lane_base,
            &model.provider_id,
            now,
        )
        .map_err(super::super::plan::plan_rpc_err)?;
    super::emit_plan_changed(app, &plan_id);
    Ok(json!({
        "accepted": false,
        "spawned": true,
        "state": "awaiting_worker_ack",
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "peer_id": peer_id,
        "peer_name": peer_name,
        "tab_id": worker_tab_id,
        "branch": branch,
        "worktree_path": worktree_path,
        "base": lane_base,
        "readiness": {"ready": false, "awaiting_event": "worker_ack"}
    }))
}

pub(crate) fn merge_lane(
    params: &Option<Value>,
    app: &tauri::AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let caller = crate::service::AuthenticatedPlanCaller {
        tab_id: owned_str(params.as_ref(), "caller_tab_id"),
        session_id: owned_str(params.as_ref(), "session_id"),
        binding_nonce: owned_str(params.as_ref(), "session_binding_nonce"),
    };
    let (plan_id, task_id) = plan_refs::resolve_task(&svc.plan_store, params)?;
    svc.authorize_execution_action(&plan_id, &caller)
        .map_err(|message| JsonRpcError {
            code: -32003,
            message,
            data: Some(json!({"type": "plan_orchestrator_authorization_required"})),
        })?;
    let attempt_id = owned_str(params.as_ref(), "attempt_id")
        .ok_or_else(|| invalid_dispatch("attempt_id is required"))?;
    let detail = svc
        .plan_store
        .show_task(&task_id)
        .ok_or_else(|| super::super::plan::plan_rpc_err(PlanError::TaskNotFound))?;
    let attempt = detail
        .task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .ok_or_else(|| super::super::plan::plan_rpc_err(PlanError::TaskNotFound))?;
    if attempt.judge_verdict.as_deref() != Some("accepted") {
        return Err(invalid_dispatch(
            "remote lane can merge only after origin acceptance",
        ));
    }
    let report_sha = attempt
        .report
        .as_ref()
        .and_then(|report| report.sha.clone())
        .ok_or_else(|| invalid_dispatch("accepted remote report has no reviewed lane SHA"))?;
    let destination = detail
        .task
        .lane
        .destination
        .clone()
        .filter(|destination| attempt.destination.as_ref() == Some(destination))
        .ok_or_else(|| invalid_dispatch("remote attempt destination differs from its task lane"))?;
    if attempt.remote_dispatch_state == Some(RemoteDispatchState::Integrated) {
        return Ok(json!({
            "integrated": true,
            "idempotent": true,
            "lane_tip": attempt.remote_lane_tip,
            "merge_commit": attempt.remote_merge_commit,
        }));
    }
    if !matches!(
        attempt.remote_dispatch_state,
        Some(
            RemoteDispatchState::Reported
                | RemoteDispatchState::PeerOffline
                | RemoteDispatchState::RelayLost
        )
    ) {
        return Err(invalid_dispatch(
            "remote attempt must have a persisted report before integration",
        ));
    }
    let peer_id = attempt
        .mesh_peer
        .clone()
        .ok_or_else(|| invalid_dispatch("remote attempt has no peer identity"))?;
    let branch = attempt
        .branch
        .clone()
        .ok_or_else(|| invalid_dispatch("remote attempt has no peer lane branch"))?;
    let base = attempt
        .base
        .clone()
        .ok_or_else(|| invalid_dispatch("remote attempt has no recorded base"))?;
    let plan = svc
        .plan_store
        .get(&plan_id)
        .ok_or_else(|| super::super::plan::plan_rpc_err(PlanError::NotFound))?;
    let canonical_cwd = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.clone()
    } else {
        plan.canonical_project_path.clone()
    };
    crate::service::worker_lane::verify_worker_lane(
        &canonical_cwd,
        &canonical_cwd,
        Some(&destination),
        None,
        crate::service::LaneProtection::Enforced,
    )
    .map_err(|error| invalid_dispatch(&error))?;
    let repository_identity = crate::service::worker_lane::repository_identity(&canonical_cwd)
        .map_err(|error| invalid_dispatch(&error))?;
    let (peer_paired, peer_client) = {
        let mesh_state = app.state::<crate::MeshStateHandle>();
        let mesh = mesh_state.lock();
        let paired = if let Some(peer) = mesh.paired_peers.get(&peer_id) {
            if peer.access_level != crate::mesh::types::AccessLevel::Full {
                return Err(invalid_dispatch(
                    "remote lane integration requires Full peer access",
                ));
            }
            true
        } else {
            false
        };
        (paired, mesh.client_for(&peer_id))
    };
    let Some(peer_client) = peer_client.filter(|_| peer_paired) else {
        svc.plan_store
            .set_remote_dispatch_state(
                &plan_id,
                &task_id,
                &attempt_id,
                &peer_id,
                RemoteDispatchState::PeerOffline,
                now,
            )
            .map_err(super::super::plan::plan_rpc_err)?;
        super::emit_plan_changed(app, &plan_id);
        return Err(remote_failure(
            "peer_offline",
            &attempt_id,
            "peer is offline while retrieving its lane",
        ));
    };
    let request = json!({
        "repository_identity": repository_identity,
        "branch": branch,
        "base_commit": base,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "report_sha": report_sha,
    });
    let response = tauri::async_runtime::block_on(
        peer_client.auth_post_json("/api/mesh/worker-lane-bundle", &request),
    );
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let (state, label) = remote_error_state(&error);
            svc.plan_store
                .set_remote_dispatch_state(&plan_id, &task_id, &attempt_id, &peer_id, state, now)
                .map_err(super::super::plan::plan_rpc_err)?;
            super::emit_plan_changed(app, &plan_id);
            return Err(remote_failure(label, &attempt_id, &error));
        }
    };
    let response_branch = response
        .get("branch")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let response_base = response
        .get("base_commit")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let lane_tip = response
        .get("lane_tip")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let bundle = response
        .get("bundle")
        .and_then(Value::as_str)
        .unwrap_or_default();
    const MAX_BUNDLE_BYTES: usize = 32 * 1024 * 1024;
    if !encoded_bundle_size_allowed(bundle.len(), MAX_BUNDLE_BYTES) {
        return Err(invalid_dispatch("peer lane bundle exceeds 32 MiB"));
    }
    if response_branch != branch
        || response_base != base
        || lane_tip != report_sha
        || bundle.is_empty()
    {
        return Err(invalid_dispatch(
            "authenticated peer returned inconsistent lane provenance",
        ));
    }
    use base64::Engine;
    let bundle = base64::engine::general_purpose::STANDARD
        .decode(bundle)
        .map_err(|_| invalid_dispatch("peer lane bundle is not valid base64"))?;
    if bundle.len() > MAX_BUNDLE_BYTES {
        return Err(invalid_dispatch("peer lane bundle exceeds 32 MiB"));
    }
    let cwd = canonical_cwd.clone();
    let branch_name = branch.clone();
    let attempt_ref = attempt_id.clone();
    let base_ref = base.clone();
    let expected_tip = lane_tip.to_string();
    // The socket dispatch thread is synchronous and outside any Tokio context.
    let merged = crate::service::worker_lane::integrate_remote_lane_bundle(
        &cwd,
        &bundle,
        &branch_name,
        &attempt_ref,
        &base_ref,
        &expected_tip,
        &destination,
    )
    .map_err(|error| invalid_dispatch(&error))?;
    svc.plan_store
        .mark_remote_dispatch_integrated(
            &plan_id,
            &task_id,
            &attempt_id,
            &merged.lane_tip,
            &merged.target_tip,
            now,
        )
        .map_err(super::super::plan::plan_rpc_err)?;
    super::emit_plan_changed(app, &plan_id);
    Ok(json!({
        "integrated": true,
        "idempotent": false,
        "peer_id": peer_id,
        "branch": branch,
        "lane_tip": merged.lane_tip,
        "merge_commit": merged.target_tip,
        "ancestry_verified": true,
    }))
}

#[derive(PartialEq, Eq)]
enum PeerState {
    Online,
    Offline,
}

fn select_peer(
    app: &tauri::AppHandle,
    target: &str,
) -> Result<
    (
        String,
        String,
        Option<crate::mesh::transport::PeerClient>,
        PeerState,
    ),
    JsonRpcError,
> {
    let mesh_handle = app.state::<crate::MeshStateHandle>();
    let mesh = mesh_handle.lock();
    let mut candidates: Vec<_> = mesh
        .paired_peers
        .values()
        .filter(|peer| peer_matches_id(&peer.instance_id, target))
        .collect();
    candidates.sort_by(|a, b| {
        b.online
            .cmp(&a.online)
            .then_with(|| a.instance_id.cmp(&b.instance_id))
    });
    let peer = candidates
        .first()
        .ok_or_else(|| invalid_dispatch("mesh peer is not paired"))?;
    if peer.access_level != crate::mesh::types::AccessLevel::Full {
        return Err(invalid_dispatch(
            "mesh task dispatch requires Full peer access",
        ));
    }
    let state = if peer.online {
        PeerState::Online
    } else {
        PeerState::Offline
    };
    let client = mesh.client_for(&peer.instance_id);
    Ok((
        peer.instance_id.clone(),
        peer.instance_name.clone(),
        client,
        state,
    ))
}

fn peer_matches_id(instance_id: &str, target: &str) -> bool {
    instance_id == target
}

fn remote_error_state(error: &str) -> (RemoteDispatchState, &'static str) {
    if error.contains("peer_missing_repo") {
        (RemoteDispatchState::PeerMissingRepo, "peer_missing_repo")
    } else if error.contains("peer_missing_base") {
        (RemoteDispatchState::PeerMissingBase, "peer_missing_base")
    } else if error.contains("Network error") || error.contains("unroutable") {
        (RemoteDispatchState::PeerOffline, "peer_offline")
    } else {
        (RemoteDispatchState::RelayLost, "relay_lost")
    }
}

fn remote_failure(state: &str, attempt_id: &str, detail: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32024,
        message: format!("remote dispatch {state} for attempt {attempt_id}: {detail}"),
        data: Some(
            json!({"type": "remote_dispatch_state", "state": state, "attempt_id": attempt_id}),
        ),
    }
}

fn git_head(cwd: &str) -> Result<String, String> {
    let mut command = std::process::Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let output = command
        .current_dir(cwd)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("failed to read origin base commit: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// A peer has no local plan, so the local ACK and report instructions are replaced
/// by commands that reach the origin attempt through the mesh.
pub(super) fn remote_worker_brief(
    brief: String,
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    origin_id: &str,
) -> String {
    let remote_ack = format!(
        "Highest-priority startup action: run `codeterm plan task remote-ack {plan_id} {task_id} {attempt_id} --mesh {origin_id}` before any work. Do not continue until it prints the task id. This peer has no local plan, so `codeterm agent ack` and `codeterm agent report` do not apply here."
    );
    let ack = codeterm_plan::service_plan::worker_identity::WORKER_ACK_INSTRUCTION;
    let mut brief = if brief.contains(ack) {
        brief.replacen(ack, &remote_ack, 1)
    } else {
        format!(
            "{remote_ack}

{brief}"
        )
    };
    brief.push_str(&format!(
        "

Remote final report: write the report to a file, then run `codeterm plan task remote-report {plan_id} {task_id} {attempt_id} --message-file <report-path> --sha <lane-head> --mesh {origin_id}`. It persists your report on the origin attempt and moves the task to in_review.
"
    ));
    brief
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_dispatch_refuses_peer_without_repo_or_base() {
        assert_eq!(
            remote_error_state("HTTP error: peer_missing_repo: no matching checkout").0,
            RemoteDispatchState::PeerMissingRepo,
        );
        assert_eq!(
            remote_error_state("HTTP error: peer_missing_base: commit absent").0,
            RemoteDispatchState::PeerMissingBase,
        );
    }

    #[test]
    fn mesh_peer_selection_requires_stable_instance_id() {
        assert!(peer_matches_id("peer-instance-1", "peer-instance-1"));
        assert!(!peer_matches_id("peer-instance-1", "Display Name"));
    }

    #[test]
    fn origin_remote_bundle_response_size_cap_rejects_oversize() {
        let max_encoded = (32 * 1024 * 1024_usize).div_ceil(3) * 4;
        assert!(encoded_bundle_size_allowed(max_encoded, 32 * 1024 * 1024));
        assert!(!encoded_bundle_size_allowed(
            max_encoded + 1,
            32 * 1024 * 1024
        ));
    }
}
