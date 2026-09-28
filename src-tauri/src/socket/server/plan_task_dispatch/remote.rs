use serde_json::{json, Value};
use tauri::Manager;

use crate::plan::RemoteDispatchState;
use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::{dispatch_authorized_gates, invalid_dispatch, owned_str};
use crate::socket::server::{plan_refs, JsonRpcError};

pub(super) fn dispatch(
    params: &Option<Value>,
    app: &tauri::AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let peer_target = owned_str(p, "mesh_peer")
        .filter(|peer| !peer.trim().is_empty())
        .ok_or_else(|| invalid_dispatch("mesh dispatch requires a peer name or id"))?;
    let (plan_id, task_id) = plan_refs::resolve_task(&svc.plan_store, params)?;
    let detail = svc
        .plan_store
        .show_task(&plan_id, &task_id)
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
        crate::service::LaneProtection::Waived,
    )
    .map_err(|error| invalid_dispatch(&error))?;
    let base = git_head(&canonical_cwd).map_err(|error| invalid_dispatch(&error))?;
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
        super::RegressionDispatchDecision::Blocked { blocking, retrospectives } => {
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
    let manifest = crate::service::plan::plan_skills::plan_skill_manifest(&svc.plan_store, &plan_id)
        .map_err(super::super::plan::plan_rpc_err)?;
    let mut brief = super::brief::build_brief_with_manifest(
        &detail.task,
        &detail.sections,
        &plan_id,
        &manifest,
        &authorized_gates,
    );
    brief.push_str(&format!(
        "\n\nRemote assignment ACK:\ncodeterm plan task remote-ack {} {} {} --mesh {}\n",
        plan_id, task_id, attempt_id, peer_name
    ));
    brief.push_str(&format!(
        "Remote final report: run `codeterm plan task remote-report {} {} {} --message-file <report-path> --sha <lane-head> --mesh {}`. It persists your report on the origin attempt and moves the task to in_review.\n",
        plan_id, task_id, attempt_id, peer_name
    ));
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
    let worker_tab_id = response
        .get("worker_tab_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            remote_failure(
                "relay_lost",
                &attempt_id,
                "peer response omitted worker_tab_id; the origin attempt is retained",
            )
        })?;
    let branch = response.get("branch").and_then(Value::as_str).unwrap_or_default();
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

#[derive(PartialEq, Eq)]
enum PeerState {
    Online,
    Offline,
}

fn select_peer(
    app: &tauri::AppHandle,
    target: &str,
) -> Result<(String, String, Option<crate::mesh::transport::PeerClient>, PeerState), JsonRpcError> {
    let mesh_handle = app.state::<crate::MeshStateHandle>();
    let mesh = mesh_handle.lock();
    let mut candidates: Vec<_> = mesh
        .paired_peers
        .values()
        .filter(|peer| peer.instance_id == target || peer.instance_name.eq_ignore_ascii_case(target))
        .collect();
    candidates.sort_by(|a, b| b.online.cmp(&a.online).then_with(|| a.instance_id.cmp(&b.instance_id)));
    let peer = candidates.first().ok_or_else(|| invalid_dispatch("mesh peer is not paired"))?;
    if peer.access_level != crate::mesh::types::AccessLevel::Full {
        return Err(invalid_dispatch("mesh task dispatch requires Full peer access"));
    }
    let state = if peer.online { PeerState::Online } else { PeerState::Offline };
    let client = mesh.client_for(&peer.instance_id);
    Ok((
        peer.instance_id.clone(),
        peer.instance_name.clone(),
        client,
        state,
    ))
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
        data: Some(json!({"type": "remote_dispatch_state", "state": state, "attempt_id": attempt_id})),
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
