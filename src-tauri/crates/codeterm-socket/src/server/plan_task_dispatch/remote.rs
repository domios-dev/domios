use serde_json::{json, Value};

use codeterm_service_core::plan::RemoteDispatchState;
use codeterm_service_core::service::team_template_remote::{self, HostedRole};
use codeterm_service_core::service::ServiceState;

use super::{dispatch_authorized_gates, invalid_dispatch, owned_str};
use crate::server::{plan_refs, JsonRpcError};

pub(super) mod brief;

// The peer creates a lane and launches the Worker before answering; a busy Windows peer
// outlives the 5s unary default, which closed live attempts as peer_offline.
const WORKER_DISPATCH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(120);
const WORKER_LANE_BUNDLE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

fn encoded_bundle_size_allowed(encoded_size: usize, max_bundle_bytes: usize) -> bool {
    encoded_size <= max_bundle_bytes.div_ceil(3) * 4
}

struct RemoteModelSelection {
    provider_id: String,
    model: Option<String>,
}

fn remote_model_selection(
    explicit_provider: Option<&str>,
    requested: Option<&str>,
    default_provider: &str,
    provider_ids: &[String],
) -> Result<RemoteModelSelection, JsonRpcError> {
    let explicit = explicit_provider
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let requested = requested.map(str::trim).filter(|value| !value.is_empty());
    let (provider, model) = match requested {
        Some(requested) if requested.contains(':') => {
            let (provider, model) = requested
                .split_once(':')
                .ok_or_else(|| invalid_dispatch("mesh model requires provider:model"))?;
            if provider.is_empty() || model.is_empty() {
                return Err(invalid_dispatch("mesh model requires provider:model"));
            }
            if explicit.is_some_and(|explicit| !explicit.eq_ignore_ascii_case(provider)) {
                return Err(invalid_dispatch(
                    "mesh model provider conflicts with explicit agent",
                ));
            }
            (provider, Some(model))
        }
        Some(requested)
            if provider_ids
                .iter()
                .any(|id| id.eq_ignore_ascii_case(requested)) =>
        {
            if explicit.is_some_and(|explicit| !explicit.eq_ignore_ascii_case(requested)) {
                return Err(invalid_dispatch(
                    "mesh model provider conflicts with explicit agent",
                ));
            }
            (requested, None)
        }
        model => (explicit.unwrap_or(default_provider), model),
    };
    Ok(RemoteModelSelection {
        provider_id: provider.to_string(),
        model: model.map(str::to_string),
    })
}

pub(super) fn dispatch(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    if owned_str(p, "sandbox").is_some() {
        return Err(invalid_dispatch("explicit sandbox selection is local-only; remote dispatch cannot drop a confinement request"));
    }
    if owned_str(p, "launch_profile").is_some() {
        return Err(invalid_dispatch(
            "launch_profile selection is local-only for task dispatch; dispatch on the provider host or use agent spawn --mesh with --profile",
        ));
    }
    let peer_target = owned_str(p, "mesh_peer")
        .filter(|peer| !peer.trim().is_empty())
        .ok_or_else(|| {
            invalid_dispatch("mesh dispatch requires a paired peer name or instance ID")
        })?;
    let (plan_id, task_id) = plan_refs::resolve_task(&svc.plan_store, params)?;
    let initial = svc
        .plan_store
        .task_dispatch_preflight_projection(&plan_id, &task_id)
        .map_err(super::super::plan::plan_rpc_err)?;
    super::preflight::preflight_dispatch_task_status(&plan_id, &initial)?;
    if owned_str(p, "execution_intent").as_deref() != Some("mutating")
        || !initial.execution_intent.requires_repository()
    {
        return Err(invalid_dispatch(
            "mesh dispatch requires a repository task with mutating execution intent",
        ));
    }
    let caller = codeterm_service_core::service::AuthenticatedPlanCaller {
        tab_id: owned_str(p, "caller_tab_id"),
        session_id: owned_str(p, "session_id"),
        binding_nonce: owned_str(p, "session_binding_nonce"),
    };
    svc.authorize_execution_action(&plan_id, &caller)
        .map_err(|error| {
            crate::server::plan_task_authority::execution_authority_error(
                error,
                Some("plan_orchestrator_authorization_required"),
            )
        })?;
    super::remote_recovery::recover_remote_attempts(svc, &plan_id, &task_id, now, false)?;
    let task = svc
        .plan_store
        .task_dispatch_preflight_projection(&plan_id, &task_id)
        .map_err(super::super::plan::plan_rpc_err)?;
    let preparation = svc
        .plan_store
        .worker_task_preparation_projection(&plan_id, &task_id)
        .map_err(super::super::plan::plan_rpc_err)?;
    let hosted_role = hosted_role(svc, p, task.owner_tab_id.as_deref())?;
    let canonical_cwd = if task.canonical_project_path.trim().is_empty() {
        task.project_path.clone()
    } else {
        task.canonical_project_path.clone()
    };
    codeterm_service_core::service::worker_lane::verify_worker_lane(
        &canonical_cwd,
        &canonical_cwd,
        None,
        None,
        codeterm_service_core::service::LaneProtection::Enforced,
    )
    .map_err(|error| invalid_dispatch(&error))?;
    let base = git_head(&canonical_cwd).map_err(|error| invalid_dispatch(&error))?;
    let destination = codeterm_service_core::service::worker_lane::current_branch(&canonical_cwd)
        .map_err(|error| invalid_dispatch(&error))?;
    if destination.is_empty() {
        return Err(invalid_dispatch(
            "mesh dispatch requires an attached destination branch",
        ));
    }
    if task
        .lane
        .destination
        .as_deref()
        .is_some_and(|requested| !requested.trim().is_empty() && requested != destination.as_str())
    {
        return Err(invalid_dispatch(
            "canonical checkout is not on the task's requested destination branch",
        ));
    }
    let repository_identity =
        codeterm_service_core::service::worker_lane::legacy_repository_identity(&canonical_cwd)
            .map_err(|error| invalid_dispatch(&error))?;
    let repository_identities =
        codeterm_service_core::service::worker_lane::repository_identities(&canonical_cwd)
            .map_err(|error| invalid_dispatch(&error))?;
    let authorized_gates = dispatch_authorized_gates(p)?;
    let requested_model = owned_str(p, "model").or_else(|| task.model.clone());
    let model = remote_model_selection(
        owned_str(p, "agent").as_deref(),
        requested_model.as_deref(),
        &svc.configuration().general_agent.agent_provider,
        &svc.agent_registry.provider_ids(),
    )?;
    svc.plan_store
        .dispatch_review_capacity_warning_projection(&plan_id, &task_id)
        .map_err(super::super::plan::plan_rpc_err)?;
    let regression_warning =
        super::selected_regression_dispatch_warning(&svc.plan_store, &plan_id)?;
    if p.and_then(|value| value.get("placement"))
        .and_then(Value::as_str)
        .is_some_and(|placement| matches!(placement, "canonical_writer" | "shared"))
    {
        return Err(invalid_dispatch("mesh Workers always require a peer lane"));
    }
    let manifest = codeterm_service_core::service::plan::plan_skills::plan_skill_manifest(
        &svc.plan_store,
        &plan_id,
    )
    .map_err(super::super::plan::plan_rpc_err)?;
    let skill_context = brief::plan_skill_context(&svc.plan_store, &plan_id, &manifest)?;
    let origin_tab_id = owned_str(p, "caller_tab_id")
        .or(task.owner_tab_id.clone())
        .ok_or_else(|| invalid_dispatch("mesh dispatch requires the origin Orchestrator tab"))?;
    let attempt_id = codeterm_service_core::plan::ids::short_id("attempt");
    let (peer_id, peer_name, peer_client, peer_state) = select_peer(svc, &peer_target)?;
    if let Some(client) = peer_client
        .as_ref()
        .filter(|_| peer_state == PeerState::Online)
    {
        require_remote_dispatch_capability(client, &peer_name, hosted_role.is_some())?;
    }
    svc.plan_store
        .begin_remote_dispatch_attempt(
            &plan_id,
            &task_id,
            &attempt_id,
            &peer_id,
            &peer_name,
            &model.provider_id,
            hosted_role.as_ref().map(|role| role.kind.as_str()),
            Some(origin_tab_id.as_str()),
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
        super::emit_plan_changed(svc, &plan_id);
        return Err(remote_failure(
            "peer_offline",
            &attempt_id,
            "paired peer is offline; the origin attempt is retained for retry",
        ));
    }
    let brief = super::brief::build_brief_with_manifest(
        (&preparation, &task),
        &plan_id,
        &[],
        &authorized_gates,
    );
    let origin_id = svc
        .ext::<codeterm_service_core::MeshStateHandle>()
        .lock()
        .ident
        .identity
        .instance_id
        .clone();
    let brief = brief::remote_worker_brief(
        brief,
        &plan_id,
        &task_id,
        &attempt_id,
        &origin_id,
        &origin_tab_id,
        &skill_context,
    );
    let brief = match &hosted_role {
        Some(role) => team_template_remote::compose_hosted_brief(&brief, role),
        None => brief,
    };
    let (explicit_persona, _) =
        super::routing::dispatch_worker_identity(preparation.personality.as_deref());
    let description_override =
        owned_str(p, "description").filter(|description| !description.trim().is_empty());
    let description = super::task_tab_description(&task.name, description_override.as_deref());
    let description = codeterm_service_core::service::normalize_tab_description(&description)
        .map_err(invalid_dispatch)?
        .ok_or_else(|| invalid_dispatch("task Worker description cannot be empty"))?;
    let request = json!({
        "repository_identity": repository_identity,
        "repository_identities": repository_identities,
        "base_commit": base,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "origin_tab_id": origin_tab_id,
        "provider": model.provider_id,
        "model": model.model,
        "reasoning_effort": owned_str(p, "reasoning_effort"),
        "speed": owned_str(p, "speed").or_else(|| preparation.speed.clone()),
        "title": explicit_persona.as_deref(),
        "description": description,
        "task_description_managed": description_override.is_none(),
        "brief": brief,
        "clone_url": team_template_remote::origin_clone_url(&canonical_cwd),
    });
    let Some(client) = peer_client else {
        return Err(remote_failure(
            "peer_offline",
            &attempt_id,
            "paired peer has no reachable signed route",
        ));
    };
    let response = codeterm_service_core::async_runtime::block_on(client.auth_post_json_within(
        "/api/mesh/worker-dispatch",
        &request,
        Some(WORKER_DISPATCH_DEADLINE),
    ));
    let response = super::super::remote_worker_policy::dispatch_response(response, || {
        codeterm_service_core::async_runtime::block_on(client.auth_post_json_within(
            "/api/mesh/worker-dispatch-status",
            &request,
            Some(WORKER_DISPATCH_DEADLINE),
        ))
        .inspect_err(|error| {
            log::debug!("[remote-dispatch] spawn observation failed for {attempt_id}: {error}");
        })
    });
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let (state, label) = remote_error_state(&error);
            svc.plan_store
                .set_remote_dispatch_state(&plan_id, &task_id, &attempt_id, &peer_id, state, now)
                .map_err(super::super::plan::plan_rpc_err)?;
            super::emit_plan_changed(svc, &plan_id);
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
        super::emit_plan_changed(svc, &plan_id);
        return Err(remote_failure(
            "relay_lost",
            &attempt_id,
            "peer response omitted worker_tab_id; the origin attempt is retained",
        ));
    };
    svc.record_product_agent_spawned_with(Some(&model.provider_id), Some("worker"));
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
        super::emit_plan_changed(svc, &plan_id);
        return Err(remote_failure(
            "relay_lost",
            &attempt_id,
            "peer response did not contain verified lane references",
        ));
    }
    let nonce = response
        .get("session_binding_nonce")
        .and_then(Value::as_str)
        .filter(|nonce| !nonce.trim().is_empty())
        .ok_or_else(|| {
            remote_failure(
                "relay_lost",
                &attempt_id,
                "peer omitted worker binding nonce",
            )
        })?;
    let worker_title = response
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .or(explicit_persona);
    svc.plan_store
        .record_remote_dispatch_lane(
            &plan_id,
            &task_id,
            &attempt_id,
            &peer_id,
            worker_tab_id,
            worker_title.as_deref(),
            branch,
            worktree_path,
            lane_base,
            &model.provider_id,
            nonce,
            now,
        )
        .map_err(super::super::plan::plan_rpc_err)?;
    svc.arm_remote_worker_ack_deadline(
        &plan_id,
        &task_id,
        &attempt_id,
        &peer_id,
        &peer_name,
        worker_tab_id,
        &origin_tab_id,
    );
    super::emit_plan_changed(svc, &plan_id);
    let mut result = json!({
        "accepted": false,
        "spawned": true,
        "state": "awaiting_worker_ack",
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "peer_id": peer_id,
        "peer_name": peer_name,
        "tab_id": worker_tab_id,
        "title": worker_title,
        "branch": branch,
        "worktree_path": worktree_path,
        "base": lane_base,
        "readiness": {"ready": false, "awaiting_event": "worker_ack"}
    });
    if let Some(warning) = regression_warning {
        result["warnings"] = json!([warning]);
    }
    if let Some(speed) = request.get("speed").and_then(Value::as_str) {
        svc.plan_store
            .update_task_speed(&plan_id, &task_id, Some(speed), now)
            .map_err(super::super::plan::plan_rpc_err)?;
    }
    Ok(result)
}

pub(crate) fn merge_lane(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let caller = codeterm_service_core::service::AuthenticatedPlanCaller {
        tab_id: owned_str(params.as_ref(), "caller_tab_id"),
        session_id: owned_str(params.as_ref(), "session_id"),
        binding_nonce: owned_str(params.as_ref(), "session_binding_nonce"),
    };
    let (plan_id, task_id) = plan_refs::resolve_task(&svc.plan_store, params)?;
    svc.authorize_execution_action(&plan_id, &caller)
        .map_err(|error| {
            crate::server::plan_task_authority::execution_authority_error(
                error,
                Some("plan_orchestrator_authorization_required"),
            )
        })?;
    let attempt_id = owned_str(params.as_ref(), "attempt_id")
        .ok_or_else(|| invalid_dispatch("attempt_id is required"))?;
    let attempt = svc
        .plan_store
        .remote_lane_integration_projection(&plan_id, &task_id, &attempt_id)
        .map_err(super::super::plan::plan_rpc_err)?;
    if attempt.judge_verdict.as_deref() != Some("accepted") {
        return Err(invalid_dispatch(
            "remote lane can merge only after origin acceptance",
        ));
    }
    let report_sha = attempt
        .report_sha
        .clone()
        .ok_or_else(|| invalid_dispatch("accepted remote report has no reviewed lane SHA"))?;
    let destination = attempt
        .task_destination
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
    let canonical_cwd = attempt.canonical_path.clone();
    codeterm_service_core::service::worker_lane::verify_worker_lane(
        &canonical_cwd,
        &canonical_cwd,
        Some(&destination),
        None,
        codeterm_service_core::service::LaneProtection::Enforced,
    )
    .map_err(|error| invalid_dispatch(&error))?;
    let repository_identity =
        codeterm_service_core::service::worker_lane::legacy_repository_identity(&canonical_cwd)
            .map_err(|error| invalid_dispatch(&error))?;
    let repository_identities =
        codeterm_service_core::service::worker_lane::repository_identities(&canonical_cwd)
            .map_err(|error| invalid_dispatch(&error))?;
    let (peer_paired, peer_client) = {
        let mesh_state = svc.ext::<codeterm_service_core::MeshStateHandle>();
        let mesh = mesh_state.lock();
        let paired = if let Some(peer) = mesh.paired_peers.get(&peer_id) {
            if peer.access_level != codeterm_service_core::mesh::types::AccessLevel::Full {
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
        super::emit_plan_changed(svc, &plan_id);
        return Err(remote_failure(
            "peer_offline",
            &attempt_id,
            "peer is offline while retrieving its lane",
        ));
    };
    let request = json!({
        "repository_identity": repository_identity,
        "repository_identities": repository_identities,
        "branch": branch,
        "base_commit": base,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "report_sha": report_sha,
    });
    let response =
        codeterm_service_core::async_runtime::block_on(peer_client.auth_post_json_within(
            "/api/mesh/worker-lane-bundle",
            &request,
            Some(WORKER_LANE_BUNDLE_DEADLINE),
        ));
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let (state, label) = remote_error_state(&error);
            svc.plan_store
                .set_remote_dispatch_state(&plan_id, &task_id, &attempt_id, &peer_id, state, now)
                .map_err(super::super::plan::plan_rpc_err)?;
            super::emit_plan_changed(svc, &plan_id);
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
    let merged = codeterm_service_core::service::worker_lane::integrate_remote_lane_bundle(
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
    super::emit_plan_changed(svc, &plan_id);
    svc.settle_lane(&plan_id, &task_id);
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
    svc: &ServiceState,
    target: &str,
) -> Result<
    (
        String,
        String,
        Option<codeterm_service_core::mesh::transport::PeerClient>,
        PeerState,
    ),
    JsonRpcError,
> {
    let mesh_handle = svc.ext::<codeterm_service_core::MeshStateHandle>();
    let mesh = mesh_handle.lock();
    let peer = resolve_dispatch_peer(&mesh, target)?;
    if peer.access_level != codeterm_service_core::mesh::types::AccessLevel::Full {
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

fn resolve_dispatch_peer<'a>(
    mesh: &'a codeterm_service_core::mesh::MeshManager,
    target: &str,
) -> Result<&'a codeterm_service_core::mesh::types::PairedPeer, JsonRpcError> {
    mesh.resolve_peer(target).map_err(|error| {
        invalid_dispatch(&format!(
            "mesh peer resolution failed: {error:?}; use a paired peer ID or name"
        ))
    })
}

/// `--role` names a worker type of the dispatching Orchestrator's Team; the origin composes
/// its guidance because the peer holds no registry for this Team. Placement on another
/// peer than its registration names is the Orchestrator's explicit alternate placement.
fn hosted_role(
    svc: &ServiceState,
    params: Option<&Value>,
    owner_tab_id: Option<&str>,
) -> Result<Option<HostedRole>, JsonRpcError> {
    let Some(role) = owned_str(params, "role").filter(|role| {
        let role = role.trim();
        !role.is_empty() && role != "worker"
    }) else {
        return Ok(None);
    };
    let parent = owned_str(params, "caller_tab_id").or_else(|| owner_tab_id.map(str::to_string));
    let team_id = parent.and_then(|tab| svc.mux.read().get_tab(&tab)?.team_id.clone());
    team_template_remote::hosted_role(team_id.as_deref(), role.trim())
        .map(Some)
        .map_err(|error| invalid_dispatch(&error))
}

/// An unreachable identify is left to the offline path; only a peer that
/// answers without the feature is refused up front, before any attempt exists.
/// Its version comes from that identify answer, never from the peer list.
fn require_remote_dispatch_capability(
    client: &codeterm_service_core::mesh::transport::PeerClient,
    peer_name: &str,
    hosts_role_type: bool,
) -> Result<(), JsonRpcError> {
    let Ok(identify) =
        codeterm_service_core::async_runtime::block_on(client.auth_get_json("/api/identify"))
    else {
        return Ok(());
    };
    if hosts_role_type {
        team_template_remote::hosting_gate(&identify, peer_name)
            .map_err(|error| invalid_dispatch(&error))?;
    }
    if identify_advertises(
        &identify,
        codeterm_service_core::ports::socket::REMOTE_TASK_DISPATCH_FEATURE,
    ) {
        return Ok(());
    }
    let version = identify
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    Err(invalid_dispatch(&format!(
        "peer '{peer_name}' (version {version}) does not advertise remote task dispatch; update the peer"
    )))
}

fn identify_advertises(identify: &Value, feature: &str) -> bool {
    identify
        .get("features")
        .and_then(Value::as_array)
        .is_some_and(|features| features.iter().any(|f| f.as_str() == Some(feature)))
}

fn remote_error_state(error: &str) -> (RemoteDispatchState, &'static str) {
    super::super::remote_worker_policy::dispatch_error_state(error)
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
    let mut command = codeterm_util::process_util::git_command();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_model_selection_defers_catalogue_validation_to_peer() {
        let selected = remote_model_selection(None, Some("grok:grok-4.7"), "claude", &[]).unwrap();
        assert_eq!(selected.provider_id, "grok");
        assert_eq!(selected.model.as_deref(), Some("grok-4.7"));
        let selected =
            remote_model_selection(Some("remote-only"), Some("unknown-model"), "claude", &[])
                .unwrap();
        assert_eq!(selected.provider_id, "remote-only");
        assert_eq!(selected.model.as_deref(), Some("unknown-model"));
        assert!(
            remote_model_selection(Some("claude"), Some("grok:grok-4.7"), "claude", &[]).is_err()
        );
        assert!(remote_model_selection(None, Some("grok:"), "claude", &[]).is_err());
        let selected =
            remote_model_selection(None, Some("codex"), "claude", &["codex".into()]).unwrap();
        assert_eq!(selected.provider_id, "codex");
        assert_eq!(selected.model, None);
    }

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
    fn mesh_remote_dispatch_peer_refusal_is_not_relay_lost() {
        for detail in [
            "HTTP error: 500 Internal Server Error: peer_worker_spawn_failed: startup supervisor unavailable",
            "HTTP error: 413 Payload Too Large: worker lane bundle exceeds 32 MiB",
        ] {
            let (state, label) = remote_error_state(detail);
            assert_eq!(state, RemoteDispatchState::PeerRefused);
            assert!(remote_failure(label, "attempt", detail).message.ends_with(detail));
        }
        assert_eq!(
            remote_error_state("Bad response: expected value").0,
            RemoteDispatchState::RelayLost,
        );
        assert_eq!(
            remote_error_state("Network error: connection refused").0,
            RemoteDispatchState::RelayLost,
        );
    }

    #[test]
    fn mesh_remote_dispatch_capability_is_read_from_identify_features() {
        let feature = codeterm_service_core::ports::socket::REMOTE_TASK_DISPATCH_FEATURE;
        let with = json!({"features": ["rest-api", feature]});
        let without = json!({"features": ["rest-api"], "version": "1.11.9"});
        assert!(identify_advertises(&with, feature));
        assert!(!identify_advertises(&without, feature));
        assert!(!identify_advertises(&json!({}), feature));
    }

    #[test]
    fn mesh_peer_selection_accepts_names_installation_ids_and_refuses_ambiguity() {
        let peer =
            |id: &str, name: &str, online: bool| codeterm_service_core::mesh::types::PairedPeer {
                instance_id: id.into(),
                instance_name: name.into(),
                online,
                addresses: vec![],
                api_port: 7685,
                public_key: "key".into(),
                paired_at: "now".into(),
                last_seen: None,
                is_primary: false,
                access_level: codeterm_service_core::mesh::types::AccessLevel::Full,
                domios_version: None,
                domios_build: None,
                web_ui: None,
                relay_protocol_version: 1,
                consecutive_misses: 0,
                last_active_ms: None,
            };
        let mut mesh = codeterm_service_core::mesh::MeshManager::new("origin".into(), 7685);
        mesh.disk_persist = false;
        mesh.paired_peers.clear();
        let inst_id = "7a835b57-1111-2222-3333-000000000001";
        let peer_id = "66c926f1-aaaa-bbbb-cccc-000000000001";
        mesh.insert_account_peer_with_installation_for_test(
            inst_id.into(),
            peer(peer_id, "ROG", true),
        );
        // Same-installation stale offline duplicate (#434): online record wins.
        mesh.insert_account_peer_with_installation_for_test(
            inst_id.into(),
            peer("id-old", "ROG", false),
        );
        for target in ["rog", peer_id, "66c926f1", inst_id, "7a835b57"] {
            assert_eq!(
                resolve_dispatch_peer(&mesh, target).unwrap().instance_id,
                peer_id,
                "{target}"
            );
        }
        assert_eq!(
            resolve_dispatch_peer(&mesh, "id-old").unwrap().instance_id,
            "id-old"
        );
        assert!(resolve_dispatch_peer(&mesh, "foreign").is_err());
        // Different installation sharing the name must refuse even when offline.
        mesh.insert_account_peer_with_installation_for_test(
            "8b946c68-2222-3333-4444-000000000002".into(),
            peer("id-other-machine", "ROG", false),
        );
        assert!(resolve_dispatch_peer(&mesh, "ROG").is_err());
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
