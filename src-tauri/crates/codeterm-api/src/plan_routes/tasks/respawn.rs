use super::*;

pub(super) const RESPAWN_ATTEMPT_FLAG: &str = "--attempt";

// `plan task respawn` requires the source attempt, so a hint without it names a
// command the CLI rejects.
pub(super) fn respawn_spawn_command(
    plan_id: &str,
    task_id: &str,
    source_attempt_id: &str,
) -> String {
    format!(
        "codeterm plan task respawn {plan_id} {task_id} {RESPAWN_ATTEMPT_FLAG} {source_attempt_id}"
    )
}

pub async fn task_respawn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
    Json(body): Json<TaskRespawnRequest>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    authorize_execution_action_http(
        &state.svc,
        &id,
        crate::tab_routes::authenticated_plan_caller(&headers),
    )
    .await?;
    verify_task_mutation_owner(
        &state.svc,
        &headers,
        &id,
        &tid,
        TaskMutationKind::Recovery,
        None,
    )
    .await?;
    let plan = state
        .svc
        .plan_store
        .get(&id)
        .ok_or_else(|| plan_err(codeterm_service_core::service::plan::PlanError::NotFound))?;
    let original_task = codeterm_service_core::plan::task_key::find_task(&plan.tasks, &tid)
        .cloned()
        .ok_or_else(|| plan_err(codeterm_service_core::service::plan::PlanError::TaskNotFound))?;
    if let Some(source) = original_task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == body.attempt_id && attempt.mesh_peer.is_some())
        .cloned()
    {
        let caller = crate::tab_routes::authenticated_plan_caller(&headers);
        let mut params = serde_json::to_value(&body)
            .map_err(|error| text_err(axum::http::StatusCode::BAD_REQUEST, error.to_string()))?;
        params["plan_id"] = serde_json::json!(id);
        params["task_id"] = serde_json::json!(tid);
        params["caller_tab_id"] = serde_json::json!(caller.tab_id);
        params["session_id"] = serde_json::json!(caller.session_id);
        params["session_binding_nonce"] = serde_json::json!(caller.binding_nonce);
        let service = state.svc.clone();
        return tokio::task::spawn_blocking(move || {
            (codeterm_service_core::ports::socket::bridge().respawn_remote)(
                &Some(params),
                &service.app_handle,
                &service,
                &id,
                &tid,
                &source,
                epoch_ms(),
            )
            .map(Json)
            .map_err(|error| text_err(axum::http::StatusCode::CONFLICT, error.message))
        })
        .await
        .map_err(|error| {
            text_err(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                error.to_string(),
            )
        })?;
    }
    if matches!(
        original_task.status,
        codeterm_service_core::plan::TaskStatus::Pending
            | codeterm_service_core::plan::TaskStatus::Blocked
    ) {
        codeterm_service_core::service::plan::review_gate::validate_dispatch_review_capacity(
            &plan,
            &original_task,
        )
        .map_err(plan_err)?;
    }
    let original_plan_mode = plan.mode.clone();
    let mut draft = state
        .svc
        .plan_store
        .build_task_respawn_draft(&id, &tid, body)
        .map_err(plan_err)?;
    let parent_tab_id = draft
        .parent_tab_id
        .clone()
        .or_else(|| plan.owner.tab_id.clone())
        .filter(|parent| !parent.trim().is_empty())
        .ok_or_else(|| {
            text_err(
                axum::http::StatusCode::BAD_REQUEST,
                "respawn requires a parent Orchestrator tab",
            )
        })?;
    draft.parent_tab_id = Some(parent_tab_id.clone());
    let lane_for_commit = original_task.lane.clone();
    let canonical_cwd = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.clone()
    } else {
        plan.canonical_project_path.clone()
    };
    let lane_protection = codeterm_service_core::service::LaneProtection::for_dispatch_intent(
        &original_task.interruption.execution_intent,
    );
    let verify_draft = draft.clone();
    let verify_canonical_cwd = canonical_cwd.clone();
    let verify_task = original_task.clone();
    let provenance = tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::plan::respawn_lane_verify_respawn_lane(
            &verify_canonical_cwd,
            &verify_task,
            &verify_draft,
        )
    })
    .await
    .map_err(|error| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("respawn lane verification task failed: {error}"),
        )
    })?
    .map_err(|error| {
        text_err(
            axum::http::StatusCode::CONFLICT,
            format!("respawn lane verification failed: {error}"),
        )
    })?;
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let spawn = state
        .svc
        .spawn_agent(
            codeterm_service_core::service::SpawnAgentParams {
                agent: Some(draft.provider.clone()),
                cwd: Some(draft.cwd.clone()),
                title: draft.title.clone(),
                title_provenance: Some(respawn_title_provenance(draft.explicit_title)),
                role: Some("Worker".into()),
                parent_tab_id: draft.parent_tab_id.clone(),
                task: Some(draft.task_prompt.clone()),
                workspace_id: draft.workspace_id.clone(),
                spawned_by_tab_id: draft.parent_tab_id.clone(),
                model: draft.model.clone(),
                planned_model: draft.planned_model.clone(),
                planned_reasoning_effort: draft.reasoning_effort.clone(),
                reasoning_effort: draft.reasoning_effort.clone(),
                task_launch_authority: Some(codeterm_service_core::service::TaskLaunchAuthority {
                    plan_id: draft.plan_id.clone(),
                    task_id: draft.task_id.clone(),
                    attempt_id: draft.attempt_id.clone(),
                    lane_base: original_task.lane.base.clone(),
                }),
                ..Default::default()
            },
            Some(ready_tx),
        )
        .await
        .map_err(|e| text_err(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e))?;
    draft.provider = spawn.agent.clone();
    draft.model = spawn.provider_resolved_model.clone();
    let owner = state.clone();
    let plan_id = id.clone();
    let task_id = tid.clone();
    let target = spawn.tab_id.clone();
    let title = spawn.title.clone();
    let (draft, created) = tokio::task::spawn_blocking(move || {
        let state = owner;
        let id = plan_id;
        let tid = task_id;
        draft.reasoning_effort = state.svc.resolved_worker_launch_effort_for_tab(&target);
        let created = record_respawn_attempt_and_persist_identity(
            &state,
            &id,
            &tid,
            &target,
            &draft,
            &title,
            epoch_ms(),
        );
        (draft, created)
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?;
    let created = match created {
        Ok(created) => created,
        Err(error) => {
            if let Err(close_error) = state.svc.close_tab_async(&spawn.tab_id).await {
                return Err(text_err(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "respawn attempt persistence failed: {error}; spawned tab compensation failed: {close_error}"
                    ),
                ));
            }
            return Err(plan_err(error));
        }
    };
    let parent_tab_id = created
        .attempt
        .orchestrator_tab_id
        .as_deref()
        .or(created.attempt.parent_tab_id.as_deref())
        .unwrap_or(&parent_tab_id)
        .to_string();
    let startup = codeterm_service_core::ports::socket::DispatchStartup {
        app: state.svc.app_handle.clone(),
        svc: state.svc.clone(),
        plan_id: id.clone(),
        task_id: tid.clone(),
        tab_id: spawn.tab_id.clone(),
        startup_attempt_id: spawn.pty_id.clone(),
        attempt_id: created.attempt.id.clone(),
        parent_tab_id,
        worker_title: spawn.title.clone(),
        original_task,
        original_plan_mode,
        lane_for_commit,
        canonical_project_path: canonical_cwd,
        provenance,
        provisioned_lane: None,
        lane_protection,
        spawn_command: respawn_spawn_command(&id, &tid, &draft.source_attempt.id),
    };
    std::thread::spawn(move || {
        (codeterm_service_core::ports::socket::bridge().run_dispatch_startup)(startup, ready_rx)
    });
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({
        "ok": true,
        "plan_id": id,
        "task_id": tid,
        "tab_id": spawn.tab_id,
        "spawn": spawn,
        "attempt": created.attempt,
        "state": "awaiting_dispatch_ready",
        "readiness": {
            "ready": false,
            "awaiting_event": "dispatch_ready",
        },
        "draft": draft
    })))
}
