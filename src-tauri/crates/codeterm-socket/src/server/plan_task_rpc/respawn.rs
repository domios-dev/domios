use super::super::plan_role_helpers::RespawnAttemptRoleExt;
use super::*;
use codeterm_service_core::multiplexer::types::TabCloseReason;
use codeterm_service_core::service::plan::{DispatchRollback, DispatchUndo, PlanStore};
use codeterm_service_core::service::{SpawnAgentParams, SpawnResult};

struct RespawnReservationTab<'a> {
    svc: &'a ServiceState,
    tab_id: String,
    cleanup_on_drop: bool,
}

impl<'a> RespawnReservationTab<'a> {
    fn new(svc: &'a ServiceState, tab_id: String) -> Self {
        Self {
            svc,
            tab_id,
            cleanup_on_drop: true,
        }
    }

    fn mark_spawned(&mut self) {
        self.cleanup_on_drop = false;
    }

    fn close(&mut self) -> Result<(), String> {
        let result = close_tab_if_present(self.svc, &self.tab_id);
        if result.is_ok() {
            self.cleanup_on_drop = false;
        }
        result
    }
}

impl Drop for RespawnReservationTab<'_> {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            if let Err(error) = close_tab_if_present(self.svc, &self.tab_id) {
                log::warn!(
                    "[task-respawn] reserved Tab cleanup failed tab={}: {error}",
                    self.tab_id
                );
            }
        }
    }
}

fn close_tab_if_present(svc: &ServiceState, tab_id: &str) -> Result<(), String> {
    if svc.mux.read().get_tab(tab_id).is_none() {
        Ok(())
    } else {
        svc.close_tab_with_reason(tab_id, TabCloseReason::SpawnRollback)
    }
}

fn roll_back_respawn_attempt<C>(
    store: &PlanStore,
    undo: &DispatchUndo,
    tab_id: &str,
    attempt_id: &str,
    close_tab: C,
    now_ms: u64,
) -> Result<(), String>
where
    C: FnOnce() -> Result<(), String>,
{
    let rollback = store
        .roll_back_task_dispatch_attempt(undo, tab_id, attempt_id, now_ms)
        .map_err(|error| error.to_string());
    let owns_tab = match &rollback {
        Ok(DispatchRollback::Superseded { live_tab_id, .. }) => {
            live_tab_id.as_deref() != Some(tab_id)
        }
        _ => true,
    };
    let mut errors = Vec::new();
    if let Err(error) = rollback {
        errors.push(format!("task restore failed: {error}"));
    }
    if owns_tab {
        if let Err(error) = close_tab() {
            errors.push(format!("Tab close failed: {error}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn verify_respawned_task_binding(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
) -> Result<(), String> {
    let binding = store
        .assigned_task_binding_projection(tab_id)
        .map_err(|error| error.to_string())?
        .map(|binding| (binding.plan_id, binding.task_id));
    match binding {
        Some((bound_plan, bound_task)) if bound_plan == plan_id && bound_task == task_id => Ok(()),
        binding => Err(format!(
            "ORCHESTRATION PRODUCT INCIDENT: respawned Worker tab '{tab_id}' has no matching \
             authoritative task binding. Resolved binding: {binding:?}"
        )),
    }
}

pub(super) fn respawn(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    respawn_with_spawn(params, svc, now, |svc, spawn_params, ready_tx| {
        svc.spawn_agent_sync_with_ready(spawn_params, ready_tx)
    })
}

fn respawn_with_spawn<S>(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
    spawn_agent: S,
) -> Result<Value, JsonRpcError>
where
    S: FnOnce(
        &ServiceState,
        SpawnAgentParams,
        Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<SpawnResult, String>,
{
    let p = params.as_ref();
    let store = &svc.plan_store;
    let (id, tid) = plan_refs::resolve_task(store, params)?;
    authorize_execution(svc, &id, p)?;
    let task = store
        .task_launch_projection(&id, &tid, None)
        .map_err(plan_rpc_err)?;
    let source_id = get_string_param(params, "attempt_id")?;
    if let Some(source) = store
        .respawn_remote_source_projection(&id, &tid, &source_id)
        .map_err(plan_rpc_err)?
    {
        return super::super::plan_task_dispatch::remote_recovery::respawn_remote(
            params, svc, &id, &tid, &source, now,
        );
    }
    if task.status == TaskStatus::Pending {
        store
            .dispatch_review_capacity_warning_projection(&id, &tid)
            .map_err(plan_rpc_err)?;
    }
    let body = p.ok_or(JsonRpcError {
        code: -32602,
        message: "params required".to_string(),
        data: None,
    })?;
    let request = codeterm_service_core::service::plan::task_attempts::TaskRespawnRequest {
        role: body.get("role").and_then(Value::as_str).map(str::to_owned),
        launch_profile: body
            .get("launch_profile")
            .and_then(Value::as_str)
            .map(str::to_owned),
        reasoning_effort: body
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .map(str::to_owned),
        attempt_id: get_string_param(params, "attempt_id")?,
        provider: body
            .get("provider")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        model: body
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        parent_tab_id: body
            .get("parent_tab_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        workspace_id: body
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        title: body
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    };
    let mut draft = store
        .build_task_respawn_draft(&id, &tid, request)
        .map_err(plan_rpc_err)?;
    svc.validate_launch_profile(&draft.provider, draft.launch_profile.as_deref())
        .map_err(super::super::model_errors::launch_profile_rpc_error)?;
    let seed = store
        .task_dispatch_seed_projection(&id, &tid, "")
        .map_err(plan_rpc_err)?;
    let launch = store
        .task_launch_projection(&id, &tid, Some(&draft.source_attempt.id))
        .map_err(plan_rpc_err)?;
    let plan = store
        .competition_lane_projection(&id, &tid)
        .map_err(plan_rpc_err)?;
    let canonical_cwd = plan.canonical_path.clone();
    let lane_protection =
        codeterm_service_core::service::LaneProtection::for_dispatch_intent(&task.dispatch_intent);
    let provenance = codeterm_service_core::service::plan::respawn_lane::verify_respawn_lane(
        &canonical_cwd,
        &launch,
        &draft,
    )
    .map_err(|error| JsonRpcError {
        code: -32603,
        message: format!("respawn lane verification failed: {error}"),
        data: None,
    })?;
    if let Some(provenance) = provenance.as_ref() {
        if task.dispatch_intent == codeterm_service_core::plan::ExecutionIntent::Mutating {
            let verdict = codeterm_service_core::service::task_lane_guard::classify_lane_facts(
                &plan.canonical_path,
                &provenance.worktree_path,
                plan.foreign_lanes
                    .iter()
                    .map(|lane| (lane.task_id.as_str(), lane.worktree_path.as_str())),
            );
            super::super::plan_task_dispatch::lane_safety::refuse_unsafe_mutating_lane(
                &plan.id, &tid, verdict,
            )?;
        }
    }
    let parent_tab_id = draft
        .parent_tab_id
        .clone()
        .or_else(|| plan.owner_tab_id.clone())
        .filter(|parent| !parent.trim().is_empty())
        .ok_or(JsonRpcError {
            code: -32602,
            message: "respawn requires a parent Orchestrator tab".into(),
            data: None,
        })?;
    draft.parent_tab_id = Some(parent_tab_id.clone());
    svc.resolve_respawn_role(&mut draft)
        .map_err(|message| JsonRpcError {
            code: -32602,
            message,
            data: None,
        })?;
    let mut spawn_params = super::super::plan_task_dispatch::respawn_worker_params(
        &draft,
        &task.name,
        task.lane.base.clone(),
        &parent_tab_id,
    );
    if let Some(provenance) = provenance.as_ref() {
        spawn_params.provisioned_cwd = Some(provenance.worktree_path.clone());
    }
    spawn_params.speed = body
        .get("speed")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or(spawn_params.speed);
    svc.validate_spawn_options(&draft.provider, &spawn_params)
        .map_err(super::super::model_errors::spawn_target_rpc_error)?;
    let reservation_workspace = spawn_params
        .workspace_id
        .clone()
        .or_else(|| svc.mux.read().find_workspace_for_tab(&parent_tab_id))
        .or_else(|| {
            let mux = svc.mux.read();
            mux.get_active_workspace()
                .map(|w| w.id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
        })
        .ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "respawn could not resolve a workspace for its Worker".into(),
            data: None,
        })?;
    let respawn_reservation = store
        .reserve_task_respawn_startup(&id, &tid, &draft.source_attempt.id, &draft.attempt_id)
        .map_err(plan_rpc_err)?;
    svc.retire_respawn_executor(&draft)
        .map_err(|error| JsonRpcError {
            code: -32603,
            message: error,
            data: None,
        })?;
    let reserved_tab_id = {
        let mut mux = svc.mux.write();
        let tab_id = mux
            .create_tab(&reservation_workspace)
            .map_err(|error| JsonRpcError {
                code: -32603,
                message: format!("respawn failed at tab_reservation: {error}"),
                data: None,
            })?;
        if let Some(title) = spawn_params.title.as_deref() {
            if let Err(error) = mux.rename_tab(&tab_id, title) {
                mux.rollback_spawn_tab(&reservation_workspace, &tab_id);
                return Err(JsonRpcError {
                    code: -32603,
                    message: format!("respawn failed at tab_reservation: {error}"),
                    data: None,
                });
            }
        }
        tab_id
    };
    let mut reservation_tab = RespawnReservationTab::new(svc, reserved_tab_id.clone());
    codeterm_service_core::startup::boot_timing::bind_request(&reserved_tab_id);
    spawn_params.workspace_id = Some(reservation_workspace);
    spawn_params.existing_tab_id = Some(reserved_tab_id.clone());

    let (mut created, rollback) = match store.record_task_respawn_attempt_with_profile(
        &id,
        &tid,
        &reserved_tab_id,
        &draft,
        svc,
        now,
    ) {
        Ok(recorded) => recorded,
        Err(error) => {
            let mut rpc_error = plan_rpc_err(error);
            if let Err(cleanup) = reservation_tab.close() {
                rpc_error.message = format!(
                    "{}; reserved respawn tab cleanup failed: {cleanup}",
                    rpc_error.message
                );
            }
            return Err(rpc_error);
        }
    };
    super::super::plan_task_dispatch::todo_seed::seed_acceptance_or_rollback(
        super::super::plan_task_dispatch::todo_seed::DispatchSeed {
            svc,
            store,
            plan_id: &id,
            task_id: &tid,
            parent_tab_id: &parent_tab_id,
            tab_id: &reserved_tab_id,
            attempt_id: &created.attempt.id,
            seed: &seed,
            rollback: &rollback,
            provisioned_lane: None,
            now,
        },
    )?;

    let speed = spawn_params.speed.clone();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let spawn = match spawn_agent(svc, spawn_params, Some(ready_tx)) {
        Ok(spawn) => spawn,
        Err(error) => {
            let cleanup = roll_back_respawn_attempt(
                store,
                &rollback,
                &reserved_tab_id,
                &created.attempt.id,
                || reservation_tab.close(),
                now,
            );
            log::warn!(
                "[task-respawn] respawn_startup_failed plan={} task={} tab={} attempt={} stage=agent_spawn cause={error}",
                id,
                tid,
                reserved_tab_id,
                created.attempt.id
            );
            emit_plan_changed(svc, &id);
            return Err(JsonRpcError {
                code: -32603,
                message: match cleanup {
                    Ok(()) => error,
                    Err(cleanup) => format!("{error}; {cleanup}"),
                },
                data: None,
            });
        }
    };
    if spawn.tab_id != reserved_tab_id {
        let cause = format!(
            "spawn claimed Tab {}, expected reserved Tab {reserved_tab_id}",
            spawn.tab_id
        );
        let cleanup = roll_back_respawn_attempt(
            store,
            &rollback,
            &reserved_tab_id,
            &created.attempt.id,
            || {
                let reserved_cleanup = reservation_tab.close();
                let spawned_cleanup = close_tab_if_present(svc, &spawn.tab_id);
                reserved_cleanup.and(spawned_cleanup)
            },
            now,
        );
        log::warn!(
            "[task-respawn] respawn_startup_failed plan={} task={} tab={} attempt={} stage=tab_reservation cause={cause}",
            id,
            tid,
            reserved_tab_id,
            created.attempt.id
        );
        emit_plan_changed(svc, &id);
        return Err(JsonRpcError {
            code: -32603,
            message: match cleanup {
                Ok(()) => cause,
                Err(cleanup) => format!("{cause}; {cleanup}"),
            },
            data: None,
        });
    }
    reservation_tab.mark_spawned();
    let tab_id = spawn.tab_id.clone();
    drop(respawn_reservation);

    if let Some(speed) = speed.as_deref() {
        store
            .update_task_speed(&id, &tid, Some(speed), now)
            .map_err(plan_rpc_err)?;
    }
    draft.provider = spawn.agent.clone();
    draft.model = spawn.provider_resolved_model.clone();
    draft.reasoning_effort = svc.resolved_worker_launch_effort_for_tab(&tab_id);
    match store.update_respawn_attempt_launch_identity(
        &id,
        &tid,
        &tab_id,
        &created.attempt.id,
        &spawn.agent,
        spawn.provider_resolved_model.clone(),
        &spawn.title,
        now,
    ) {
        Ok(Some(updated)) => created.attempt = updated,
        Ok(None) => {}
        Err(error) => {
            log::warn!(
                "[task-respawn] attempt_launch_identity_not_recorded plan={} task={} tab={} attempt={} cause={error}",
                id,
                tid,
                tab_id,
                created.attempt.id
            );
        }
    }
    if let Err(error) = svc.persist_dispatched_attempt_reasoning_effort(
        &id,
        &tid,
        &tab_id,
        &created.attempt.id,
        now,
    ) {
        log::warn!(
            "[task-respawn] attempt_reasoning_effort_not_recorded plan={} task={} tab={} attempt={} cause={error}",
            id,
            tid,
            tab_id,
            created.attempt.id
        );
    }
    if let Err(error) = verify_respawned_task_binding(store, &id, &tid, &tab_id) {
        let cleanup = roll_back_respawn_attempt(
            store,
            &rollback,
            &tab_id,
            &created.attempt.id,
            || close_tab_if_present(svc, &tab_id),
            now,
        );
        log::warn!(
            "[task-respawn] respawn_startup_failed plan={} task={} tab={} attempt={} stage=task_binding cause={error}",
            id,
            tid,
            tab_id,
            created.attempt.id
        );
        emit_plan_changed(svc, &id);
        return Err(JsonRpcError {
            code: -32603,
            message: match cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; {cleanup}"),
            },
            data: None,
        });
    }
    svc.persist_current_attempt_session_model(&tab_id);
    emit_plan_changed(svc, &id);
    let startup = super::super::plan_task_dispatch::DispatchStartup {
        svc: svc.clone(),
        plan_id: id.clone(),
        task_id: tid.clone(),
        tab_id: tab_id.clone(),
        startup_attempt_id: spawn.pty_id.clone(),
        attempt_id: created.attempt.id.clone(),
        parent_tab_id,
        worker_title: spawn.title.clone(),
        rollback,
        lane_for_commit: task.lane.clone(),
        canonical_project_path: canonical_cwd.clone(),
        provenance,
        provisioned_lane: None,
        lane_protection,
        spawn_command: format!(
            "domios plan task respawn {id} {tid} --attempt {}",
            draft.source_attempt.id
        ),
    };
    std::thread::spawn(move || {
        super::super::plan_task_dispatch::startup::run_startup(startup, ready_rx)
    });
    Ok(json!({
        "ok": true,
        "accepted": true,
        "state": "awaiting_dispatch_ready",
        "readiness": {
            "ready": false,
            "awaiting_event": "dispatch_ready",
        },
        "plan_id": id,
        "task_id": tid,
        "tab_id": tab_id,
        "spawn": spawn,
        "attempt": created.attempt,
        "draft": draft
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::plan::{
        ExecutionIntent, OrchestratorRef, PlanMode, PlanningState, TaskExecutionIntent, TaskStatus,
    };
    use codeterm_service_core::service::plan::NewTask;
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    use std::cell::Cell;
    use std::sync::Arc;

    #[test]
    fn respawn_persists_reserved_tab_and_attempt_before_spawn_and_rolls_back_on_refusal() {
        let (mut mux, _) = MultiplexerState::new();
        let workspace = mux.list_workspaces()[0].id.clone();
        let owner = mux.create_tab(&workspace).unwrap();
        mux.set_orchestrator_role(&owner).unwrap();
        let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
        {
            let tab = mux.get_tab_mut(&owner).unwrap();
            tab.agent_session_id = Some("owner-session".into());
            tab.session_binding_nonce = Some("owner-nonce".into());
            tab.reported_session_binding_nonce = Some("owner-nonce".into());
        }
        let fixture = ServiceFixture::new(mux);
        let svc = &fixture.service;
        svc.agent_registry.register_provider(Box::new(
            codeterm_service_core::agent::test_provider::MockProvider::new("fixture-agent"),
        ));
        let mut plan = svc
            .plan_store
            .create_plan_for_team(
                "respawn pre-spawn".into(),
                fixture.temp.path().display().to_string(),
                OrchestratorRef {
                    tab_id: Some(owner.clone()),
                    session_id: Some("owner-session".into()),
                    title: None,
                },
                team_id,
                vec![],
                1,
            )
            .unwrap();
        plan.mode = PlanMode::Approved;
        plan.planning_state = PlanningState::Approved;
        svc.plan_store.save(&mut plan, 2).unwrap();
        let task_id = svc
            .plan_store
            .add_task(
                &plan.id,
                NewTask {
                    name: "Respawn pre-spawn task".into(),
                    personality: Some("Beacon".into()),
                    execution_intent: TaskExecutionIntent::NonRepository,
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        svc.plan_store
            .assign_task(&plan.id, &task_id, "source-worker", None, 4)
            .unwrap();
        svc.plan_store
            .edit_plan_fixture(&plan.id, 5, |plan| {
                let task = plan.tasks.iter_mut().find(|t| t.id == task_id).unwrap();
                task.status = TaskStatus::InProgress;
                task.interruption.execution_intent = ExecutionIntent::ReadOnly;
                task.execution_attempts[0].execution_intent = Some(ExecutionIntent::ReadOnly);
                Ok(())
            })
            .unwrap();
        let source_attempt = svc
            .plan_store
            .list_task_attempts(&plan.id, &task_id)
            .unwrap()[0]
            .attempt
            .clone();
        svc.plan_store
            .record_task_executor_termination(&plan.id, &task_id, &source_attempt, 6)
            .unwrap();
        let source_id = source_attempt.id;
        let tabs_before = svc.mux.read().snapshot().tabs.len();
        let observed = Cell::new(false);

        let error = respawn_with_spawn(
            &Some(json!({
                "plan_id": plan.id,
                "task_id": task_id,
                "attempt_id": source_id,
                "provider": "fixture-agent",
                "workspace_id": workspace,
                "caller_tab_id": owner,
                "session_id": "owner-session",
                "session_binding_nonce": "owner-nonce",
            })),
            &Arc::new(svc.clone()),
            10,
            |service, params, _ready_tx| {
                let reserved_tab = params
                    .existing_tab_id
                    .as_deref()
                    .expect("respawn must reserve a tab before spawn");
                let authority = params
                    .task_launch_authority
                    .as_ref()
                    .expect("respawn must carry launch authority");
                assert_eq!(params.plan_id.as_deref(), Some(plan.id.as_str()));
                assert_eq!(params.task_id.as_deref(), Some(task_id.as_str()));
                assert!(params.plan_skill_manifest.is_some());
                assert!(
                    service.mux.read().get_tab(reserved_tab).is_some(),
                    "reserved respawn tab must exist before spawn"
                );
                let task = service
                    .plan_store
                    .inspect_task_for_test(&task_id)
                    .unwrap()
                    .task;
                assert_eq!(task.assigned_tab_id.as_deref(), Some(reserved_tab));
                assert_eq!(task.status_label.as_deref(), Some("starting"));
                assert_eq!(task.execution_attempts.len(), 2);
                assert!(task.execution_attempts[0].retired_at.is_some());
                assert!(task.execution_attempts[0].closed_at.is_some());
                let replacement = &task.execution_attempts[1];
                assert_eq!(replacement.id, authority.attempt_id);
                assert_eq!(replacement.executor_tab_id, reserved_tab);
                assert_eq!(
                    replacement.execution_intent,
                    Some(ExecutionIntent::ReadOnly)
                );
                let binding = service
                    .plan_store
                    .assigned_task_binding_projection(reserved_tab)
                    .unwrap()
                    .expect("reserved respawn tab must already carry authoritative task binding");
                assert_eq!(binding.plan_id, plan.id);
                assert_eq!(binding.task_id, task_id);
                observed.set(true);
                Err("opaque respawn spawn refusal".into())
            },
        )
        .unwrap_err();

        assert!(observed.get());
        assert_eq!(error.code, -32603);
        assert!(error.message.contains("opaque respawn spawn refusal"));
        assert_eq!(svc.mux.read().snapshot().tabs.len(), tabs_before);
        let rolled_back = svc.plan_store.inspect_task_for_test(&task_id).unwrap().task;
        assert_eq!(rolled_back.status, TaskStatus::InProgress);
        assert_eq!(
            rolled_back.assigned_tab_id.as_deref(),
            Some("source-worker")
        );
        assert_eq!(rolled_back.execution_attempts[0].retired_at, None);
        assert_eq!(rolled_back.execution_attempts[0].closed_at, None);
        assert_eq!(rolled_back.execution_attempts[1].closed_at, Some(10));
    }
}
