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
        "domios plan task respawn {plan_id} {task_id} {RESPAWN_ATTEMPT_FLAG} {source_attempt_id}"
    )
}

pub(super) fn respawn_spawn_params(
    draft: &codeterm_service_core::service::plan::task_attempts::TaskRespawnDraft,
    task_name: &str,
    lane_base: Option<String>,
) -> codeterm_service_core::service::SpawnAgentParams {
    let description = codeterm_service_core::service::task_tab_description(task_name, None);
    codeterm_service_core::service::SpawnAgentParams {
        agent: Some(draft.provider.clone()),
        cwd: Some(draft.cwd.clone()),
        title: draft.title.clone(),
        title_provenance: Some(respawn_title_provenance(draft.explicit_title)),
        description: Some(description),
        task_description_managed: true,
        role: Some("Worker".into()),
        execution_intent: Some(draft.execution_intent.clone()),
        parent_tab_id: draft.parent_tab_id.clone(),
        task: Some(draft.task_prompt.clone()),
        workspace_id: draft.workspace_id.clone(),
        spawned_by_tab_id: draft.parent_tab_id.clone(),
        model: draft.model.clone(),
        launch_profile: draft.launch_profile.clone(),
        planned_model: draft.planned_model.clone(),
        planned_reasoning_effort: draft.reasoning_effort.clone(),
        reasoning_effort: draft.reasoning_effort.clone(),
        task_launch_authority: Some(codeterm_service_core::service::TaskLaunchAuthority {
            plan_id: draft.plan_id.clone(),
            task_id: draft.task_id.clone(),
            attempt_id: draft.attempt_id.clone(),
            lane_base,
        }),
        ..Default::default()
    }
}

pub async fn task_respawn(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::ActorHeaders,
    Path((id, tid)): Path<(String, String)>,
    Json(body): Json<TaskRespawnRequest>,
) -> PlanRouteResult {
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
    let store = state.svc.plan_store.clone();
    let plan_id = id.clone();
    let task_id = tid.clone();
    let source_id = body.attempt_id.clone();
    let (original_task, remote_source, plan, owner_tab_id) =
        crate::plan_routes::run_blocking(move || {
            let task = store
                .task_launch_projection(&plan_id, &task_id, Some(&source_id))
                .map_err(plan_err)?;
            let remote = store
                .respawn_remote_source_projection(&plan_id, &task_id, &source_id)
                .map_err(plan_err)?;
            let plan = store.document_metadata(&plan_id).map_err(plan_err)?;
            let owner = store
                .plan_owner_tab_projection(&plan_id)
                .map_err(plan_err)?;
            Ok((task, remote, plan, owner))
        })
        .await?;
    if let Some(source) = remote_source {
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
        let store = state.svc.plan_store.clone();
        let plan_id = id.clone();
        let task_id = tid.clone();
        crate::plan_routes::run_blocking(move || {
            store
                .dispatch_review_capacity_warning_projection(&plan_id, &task_id)
                .map_err(plan_err)
        })
        .await?;
    }
    let store = state.svc.plan_store.clone();
    let plan_id = id.clone();
    let task_id = tid.clone();
    let (mut draft, acceptance_criteria) = crate::plan_routes::run_blocking(move || {
        let draft = store
            .build_task_respawn_draft(&plan_id, &task_id, body)
            .map_err(plan_err)?;
        let criteria = store
            .task_respawn_acceptance_projection(&plan_id, &task_id)
            .map_err(plan_err)?;
        Ok((draft, criteria))
    })
    .await?;
    let parent_tab_id = draft
        .parent_tab_id
        .clone()
        .or(owner_tab_id)
        .filter(|parent| !parent.trim().is_empty())
        .ok_or_else(|| {
            text_err(
                axum::http::StatusCode::BAD_REQUEST,
                "respawn requires a parent Orchestrator tab",
            )
        })?;
    draft.parent_tab_id = Some(parent_tab_id.clone());
    let spawn_params =
        respawn_spawn_params(&draft, &original_task.name, original_task.lane.base.clone());
    crate::agent_routes::spawn_rejection::validate_spawn_options(
        &state.svc,
        &draft.provider,
        &spawn_params,
    )
    .await
    .map_err(|error| (error.status, Json(error.body)))?;
    let lane_for_commit = original_task.lane.clone();
    let canonical_cwd = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.clone()
    } else {
        plan.canonical_project_path.clone()
    };
    let lane_protection = codeterm_service_core::service::LaneProtection::for_dispatch_intent(
        &original_task.dispatch_intent,
    );
    let verify_draft = draft.clone();
    let verify_canonical_cwd = canonical_cwd.clone();
    let verify_store = state.svc.plan_store.clone();
    let provenance = tokio::task::spawn_blocking(move || {
        let verify_task = verify_store
            .task_launch_projection(
                &verify_draft.plan_id,
                &verify_draft.task_id,
                Some(&verify_draft.source_attempt.id),
            )
            .map_err(|error| error.to_string())?;
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
    let store = state.svc.plan_store.clone();
    let plan_id = id.clone();
    let task_id = tid.clone();
    let source_attempt = draft.source_attempt.id.clone();
    let attempt = draft.attempt_id.clone();
    let _respawn_reservation = crate::plan_routes::run_blocking(move || {
        store
            .reserve_task_respawn_startup(&plan_id, &task_id, &source_attempt, &attempt)
            .map_err(plan_err)
    })
    .await?;
    let retirement_service = state.svc.clone();
    let retirement_draft = draft.clone();
    tokio::task::spawn_blocking(move || {
        retirement_service.retire_respawn_executor(&retirement_draft)
    })
    .await
    .map_err(|error| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        )
    })?
    .map_err(|error| text_err(axum::http::StatusCode::CONFLICT, error))?;
    let spawn = state
        .svc
        .spawn_agent(spawn_params, Some(ready_tx))
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
    let (created, rollback) = match created {
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
    let todo_carry_over = state
        .svc
        .tab_todo_inherit_or_seed(
            &spawn.tab_id,
            &parent_tab_id,
            std::slice::from_ref(&draft.source_attempt.executor_tab_id),
            &acceptance_criteria,
        )
        .await
        .map_err(|error| {
            log::warn!(
                "[task-respawn] checklist carry-over failed plan={id} task={tid} tab={}: {error}",
                spawn.tab_id
            );
            error
        })
        .err();
    let startup = codeterm_service_core::ports::socket::DispatchStartup {
        svc: state.svc.clone(),
        plan_id: id.clone(),
        task_id: tid.clone(),
        tab_id: spawn.tab_id.clone(),
        startup_attempt_id: spawn.pty_id.clone(),
        attempt_id: created.attempt.id.clone(),
        parent_tab_id,
        worker_title: spawn.title.clone(),
        rollback,
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
        "todo_carry_over_error": todo_carry_over,
        "draft": draft
    })))
}
