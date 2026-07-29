use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use crate::agent::reports::ReportDetails;
use crate::plan::{Task, TaskLaneMetadata};
use crate::service::plan::milestone_gates::{
    regression_dispatch_decision, DispatchExemption, RegressionDispatchDecision, RegressionGate,
};
use crate::service::plan::task_queries::TaskSection;
use crate::service::plan::PlanError;
use crate::service::{
    verify_worker_lane, ProvisionedWorkerLane, ServiceState, SpawnAgentParams,
    SpawnIdentityProvenance, WorkerLaneProvenance,
};

use super::plan::{emit_plan_changed, plan_rpc_err};
use super::plan_refs;
use super::JsonRpcError;

fn owned_str(p: Option<&Value>, key: &str) -> Option<String> {
    p.and_then(|x| x.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

const DISPATCH_READY_TIMEOUT: Duration = Duration::from_secs(60);

fn dispatch_failed(stage: &str, message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: format!("dispatch failed at {stage}: {message}"),
        data: Some(json!({
            "type": "task_dispatch_failed",
            "stage": stage,
            "reason": message,
        })),
    }
}

fn preflight_review_capacity(plan: &crate::plan::Plan, task: &Task) -> Result<(), PlanError> {
    crate::service::plan::review_gate::validate_dispatch_review_capacity(plan, task)
}

fn invalid_dispatch(reason: impl Into<String>) -> JsonRpcError {
    let reason = reason.into();
    JsonRpcError {
        code: -32602,
        message: format!("invalid task dispatch: {reason}"),
        data: Some(json!({
            "type": "invalid_task_dispatch",
            "stage": "cwd_validation",
            "reason": reason,
        })),
    }
}

#[derive(Clone, Debug)]
struct DispatchLaneResolution {
    spawn_cwd: String,
    existing: Option<WorkerLaneProvenance>,
}

fn resolve_dispatch_lane(
    explicit_cwd: Option<&str>,
    canonical_cwd: &str,
    task_lane: &TaskLaneMetadata,
) -> Result<DispatchLaneResolution, JsonRpcError> {
    if let Some(explicit) = explicit_cwd {
        if task_lane.worktree_path.as_deref().is_some_and(|recorded| {
            !crate::service::ServiceContext::paths_equal(explicit, recorded)
        }) {
            return Err(invalid_dispatch(format!(
                "explicit cwd '{}' conflicts with recorded lane worktree '{}'",
                explicit,
                task_lane.worktree_path.as_deref().unwrap_or_default()
            )));
        }
        let existing = verify_worker_lane(
            canonical_cwd,
            explicit,
            task_lane.branch.as_deref(),
            task_lane.base.as_deref(),
        )
        .map_err(invalid_dispatch)?;
        return Ok(DispatchLaneResolution {
            spawn_cwd: existing.worktree_path.clone(),
            existing: Some(existing),
        });
    }
    if let Some(recorded) = task_lane.worktree_path.as_deref() {
        let existing = verify_worker_lane(
            canonical_cwd,
            recorded,
            task_lane.branch.as_deref(),
            task_lane.base.as_deref(),
        )
        .map_err(invalid_dispatch)?;
        return Ok(DispatchLaneResolution {
            spawn_cwd: existing.worktree_path.clone(),
            existing: Some(existing),
        });
    }
    Ok(DispatchLaneResolution {
        spawn_cwd: canonical_cwd.to_string(),
        existing: None,
    })
}

fn lane_metadata_for_dispatch(
    original: &TaskLaneMetadata,
    provenance: &WorkerLaneProvenance,
) -> TaskLaneMetadata {
    let mut lane = original.clone();
    if crate::service::ServiceContext::paths_equal(
        &provenance.worktree_path,
        &provenance.canonical_cwd,
    ) {
        lane.worktree_path = None;
        lane.branch = None;
        lane.base = None;
        lane.cleanup_status = None;
        return lane;
    }
    lane.worktree_path = Some(provenance.worktree_path.clone());
    lane.branch = Some(provenance.branch.clone());
    lane.base = Some(provenance.base.clone());
    lane.cleanup_status = None;
    lane
}

fn await_agent_ready(
    ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>,
    deadline: Instant,
) -> Result<(), String> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err("agent readiness deadline expired".into());
    }
    match tauri::async_runtime::block_on(
        async move { tokio::time::timeout(remaining, ready_rx).await },
    ) {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(error))) => Err(error),
        Ok(Err(_)) => Err("agent readiness channel closed".into()),
        Err(_) => Err("agent readiness deadline expired".into()),
    }
}

fn wait_for_tab_session(
    svc: &ServiceState,
    tab_id: &str,
    deadline: Instant,
) -> Result<String, String> {
    crate::service::general_agent::binding_wait::wait_for_session_binding(deadline, || {
        svc.mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.agent_session_id.clone())
            .filter(|session| !session.trim().is_empty())
    })
    .ok_or_else(|| {
        if svc.mux.read().get_tab(tab_id).is_none() {
            format!("spawned tab '{tab_id}' was removed before session binding")
        } else {
            format!("spawned tab '{tab_id}' did not bind a session before the deadline")
        }
    })
}

fn close_tab_if_present(svc: &ServiceState, tab_id: &str) -> Result<(), String> {
    if svc.mux.read().get_tab(tab_id).is_none() {
        Ok(())
    } else {
        svc.close_tab(tab_id)
    }
}

#[derive(Clone, Debug)]
struct DispatchEvidence {
    session_id: String,
    lane: WorkerLaneProvenance,
}

fn apply_dispatch_starting_contract(
    result: &mut Value,
    tab_id: &str,
    spawn: &crate::service::SpawnResult,
) {
    let mut spawn_metadata = json!(spawn);
    if let Some(fields) = spawn_metadata.as_object_mut() {
        fields.remove("tab_id");
    }
    result["accepted"] = json!(true);
    result["state"] = json!("awaiting_dispatch_ready");
    result["readiness"] = json!({
        "ready": false,
        "awaiting_event": "dispatch_ready",
    });
    result["tab_id"] = json!(tab_id);
    result["spawn"] = spawn_metadata;
}

fn dispatch_ready_event(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt_id: &str,
    evidence: &DispatchEvidence,
) -> Value {
    json!({
        "kind": "dispatch_ready",
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
        "session_id": evidence.session_id,
        "lane": {
            "worktree_path": evidence.lane.worktree_path,
            "branch": evidence.lane.branch,
            "base": evidence.lane.base,
            "canonical_project_path": evidence.lane.canonical_cwd,
        },
    })
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn publish_startup_report(
    svc: &ServiceState,
    parent_tab_id: &str,
    worker_tab_id: &str,
    worker_title: &str,
    plan_id: &str,
    task_id: &str,
    payload: Value,
) {
    let message = payload.to_string();
    let details = ReportDetails {
        plan_id: Some(plan_id.to_string()),
        task_id: Some(task_id.to_string()),
        ..Default::default()
    };
    let report = svc.report_store.lock().add_detailed(
        parent_tab_id,
        worker_tab_id,
        worker_title,
        &message,
        details,
        None,
    );
    if !svc.report_store.lock().claim(&report.id) {
        return;
    }
    svc.events.broadcast(
        "agent.report",
        json!({
            "target_tab_id": parent_tab_id,
            "report": report.clone(),
        }),
    );
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "agent-report",
        json!({
            "target_tab_id": parent_tab_id,
            "from_tab_id": worker_tab_id,
            "from_title": worker_title,
            "message": message,
            "report": report.clone(),
        }),
    );
    let envelope = crate::service::messenger::MessageEnvelope::report(worker_title, worker_tab_id);
    if let Err(error) = svc.gateway_deliver(
        parent_tab_id,
        envelope,
        &report.inject_body(),
        crate::service::message_gateway::DeliveryClass::AgentTraffic,
    ) {
        svc.report_store.lock().release_delivery(&report.id);
        log::warn!(
            "[dispatch-startup] parent delivery failed task={} tab={}: {}",
            task_id,
            worker_tab_id,
            error
        );
    } else {
        svc.report_store.lock().mark_delivered(&report.id);
    }
}

struct DispatchStartup {
    app: AppHandle,
    svc: ServiceState,
    plan_id: String,
    task_id: String,
    tab_id: String,
    attempt_id: String,
    parent_tab_id: String,
    worker_title: String,
    original_task: Task,
    original_plan_mode: crate::plan::PlanMode,
    lane_for_commit: TaskLaneMetadata,
    provenance: WorkerLaneProvenance,
    provisioned_lane: Option<ProvisionedWorkerLane>,
}

impl DispatchStartup {
    fn run(self, ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>) {
        let deadline = Instant::now() + DISPATCH_READY_TIMEOUT;
        let provisioned_lane = self.provisioned_lane;
        let outcome = complete_dispatch_transaction(
            deadline,
            |deadline| await_agent_ready(ready_rx, deadline),
            |deadline| wait_for_tab_session(&self.svc, &self.tab_id, deadline),
            || {
                verify_worker_lane(
                    &self.provenance.canonical_cwd,
                    &self.provenance.worktree_path,
                    Some(&self.provenance.branch),
                    Some(&self.provenance.base),
                )
            },
            |lane, session_id| {
                let lane_metadata = lane_metadata_for_dispatch(&self.lane_for_commit, lane);
                self.svc
                    .plan_store
                    .finish_task_dispatch(
                        &self.plan_id,
                        &self.task_id,
                        &self.tab_id,
                        &self.attempt_id,
                        session_id,
                        lane_metadata,
                        epoch_ms(),
                    )
                    .map_err(|error| error.to_string())
            },
            || {
                rollback_failed_dispatch(
                    provisioned_lane,
                    || {
                        self.svc
                            .plan_store
                            .restore_task_dispatch_snapshot(
                                &self.plan_id,
                                &self.original_task,
                                self.original_plan_mode,
                                epoch_ms(),
                            )
                            .map_err(|error| error.to_string())
                    },
                    || close_tab_if_present(&self.svc, &self.tab_id),
                )
            },
        );

        match outcome {
            Ok(evidence) => {
                self.svc.gateway.require_worker_report(&self.tab_id);
                emit_plan_changed(&self.app, &self.plan_id);
                publish_startup_report(
                    &self.svc,
                    &self.parent_tab_id,
                    &self.tab_id,
                    &self.worker_title,
                    &self.plan_id,
                    &self.task_id,
                    dispatch_ready_event(
                        &self.plan_id,
                        &self.task_id,
                        &self.tab_id,
                        &self.attempt_id,
                        &evidence,
                    ),
                );
            }
            Err(error) => {
                let stage = error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("stage"))
                    .and_then(Value::as_str)
                    .unwrap_or("startup")
                    .to_string();
                let reason = error.message.clone();
                emit_plan_changed(&self.app, &self.plan_id);
                publish_startup_report(
                    &self.svc,
                    &self.parent_tab_id,
                    &self.tab_id,
                    &self.worker_title,
                    &self.plan_id,
                    &self.task_id,
                    json!({
                        "kind": "dispatch_startup_failed",
                        "plan_id": self.plan_id,
                        "task_id": self.task_id,
                        "tab_id": self.tab_id,
                        "attempt_id": self.attempt_id,
                        "stage": stage,
                        "error": reason,
                        "cleanup": {"ok": stage != "compensation"},
                    }),
                );
            }
        }
    }
}

fn complete_dispatch_transaction<Ready, Binding, Provenance, Commit, Rollback>(
    deadline: Instant,
    await_ready: Ready,
    wait_binding: Binding,
    validate_provenance: Provenance,
    commit: Commit,
    rollback: Rollback,
) -> Result<DispatchEvidence, JsonRpcError>
where
    Ready: FnOnce(Instant) -> Result<(), String>,
    Binding: FnOnce(Instant) -> Result<String, String>,
    Provenance: FnOnce() -> Result<WorkerLaneProvenance, String>,
    Commit: FnOnce(&WorkerLaneProvenance, &str) -> Result<(), String>,
    Rollback: FnOnce() -> Result<(), String>,
{
    let outcome = (|| {
        await_ready(deadline).map_err(|error| dispatch_failed("agent_ready", error))?;
        let session_id =
            wait_binding(deadline).map_err(|error| dispatch_failed("session_binding", error))?;
        let lane =
            validate_provenance().map_err(|error| dispatch_failed("provenance_readback", error))?;
        commit(&lane, &session_id).map_err(|error| dispatch_failed("task_commit", error))?;
        Ok(DispatchEvidence { session_id, lane })
    })();
    if outcome.is_err() {
        if let Err(cleanup) = rollback() {
            return Err(dispatch_failed("compensation", cleanup));
        }
    }
    outcome
}

fn gate_keys(gates: &[RegressionGate]) -> String {
    gates
        .iter()
        .map(|g| g.key.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

// Refuse the dispatch loudly, naming every blocking gate — no tab is spawned.
fn regression_blocked_error(gates: Vec<RegressionGate>) -> JsonRpcError {
    let names = gate_keys(&gates);
    JsonRpcError {
        code: -32010,
        message: format!(
            "dispatch blocked by pending regression gate(s): {names}. Run the warm integration QA to clear them (codeterm plan qa next <plan> --group <gate> --note \"<evidence>\"), tag the task regression-gate/release-blocking if it is the exception, or record an authorized override (codeterm plan qa override <plan> --reason \"<why>\")."
        ),
        data: Some(json!({
            "type": "regression_gate_blocked",
            "blocking_gates": gates,
        })),
    }
}

pub(super) struct DispatchRouting {
    pub agent: Option<String>,
    pub model: Option<String>,
    qualified_model: bool,
}

fn qualified_provider_model(model: &str, provider_ids: &[String]) -> Option<(String, String)> {
    let (provider, model) = model.split_once(':')?;
    if provider_ids
        .iter()
        .any(|registered| registered.eq_ignore_ascii_case(provider))
        && !model.trim().is_empty()
    {
        return Some((provider.to_string(), model.to_string()));
    }
    None
}

fn unqualified_model_error(
    routing: &DispatchRouting,
    provider: &str,
    notice: &crate::service::spawn::ModelFallbackNotice,
) -> Option<JsonRpcError> {
    if routing.qualified_model {
        return None;
    }
    Some(JsonRpcError {
        code: -32602,
        message: format!(
            "model '{}' is unavailable for provider '{}'. Specify <provider>:<model> instead of silently substituting '{}'.",
            notice.requested_model, provider, notice.provider_resolved_model
        ),
        data: Some(json!({
            "type": "unqualified_model_unavailable",
            "requested_model": notice.requested_model,
            "default_provider": provider,
            "suggestion": "use <provider>:<model> or pass --agent <provider>",
        })),
    })
}

fn dispatch_worker_identity(task: &Task) -> (Option<String>, Option<SpawnIdentityProvenance>) {
    let title = task
        .personality
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let provenance = if title.is_some() {
        match task.personality_provenance {
            Some(crate::plan::IdentityProvenance::UserSupplied) => {
                Some(SpawnIdentityProvenance::UserSupplied)
            }
            _ => Some(SpawnIdentityProvenance::Suggested),
        }
    } else {
        Some(SpawnIdentityProvenance::Suggested)
    };
    (title, provenance)
}

/// Resolve the provider and model a dispatch should spawn. Precedence: an explicit
/// `agent` provider wins (any model rides along verbatim); otherwise a requested
/// `<provider>:<model>` prefix is resolved against the live registry. A bare provider
/// name selects its default model; a bare model remains a request for the default
/// provider and is rejected if that provider cannot resolve it.
pub(super) fn resolve_dispatch_routing(
    explicit_agent: Option<String>,
    rpc_model: Option<String>,
    task_model: Option<String>,
    provider_ids: &[String],
) -> DispatchRouting {
    let clean = |s: Option<String>| s.filter(|v| !v.trim().is_empty());
    let requested_model = clean(rpc_model).or_else(|| clean(task_model));
    if let Some(agent) = clean(explicit_agent) {
        return DispatchRouting {
            agent: Some(agent),
            model: requested_model,
            qualified_model: false,
        };
    }
    if let Some(model) = requested_model {
        if let Some((agent, model)) = qualified_provider_model(&model, provider_ids) {
            return DispatchRouting {
                agent: Some(agent),
                model: Some(model),
                qualified_model: true,
            };
        }
        if provider_ids.iter().any(|p| p.eq_ignore_ascii_case(&model)) {
            return DispatchRouting {
                agent: Some(model),
                model: None,
                qualified_model: true,
            };
        }
        return DispatchRouting {
            agent: None,
            model: Some(model),
            qualified_model: false,
        };
    }
    DispatchRouting {
        agent: None,
        model: None,
        qualified_model: false,
    }
}

/// Atomic: spawn a Worker tab, assign the task (in_progress + lane metadata),
/// then deliver the brief. Any post-spawn failure reverts the task and closes
/// the tab, so a task is never left in_progress without a live tab.
pub(super) fn dispatch(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let p = params.as_ref();
    let (plan_id, task_id) = plan_refs::resolve_task(store, params)?;
    let detail = store
        .show_task(&task_id)
        .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?;

    // Frontier policy: a non-blocker cannot spawn while regression gates are
    // pending. Decide BEFORE any tab is created so a block leaves no orphan.
    let plan = store
        .get(&plan_id)
        .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
    let mut exemption: Option<DispatchExemption> = None;
    match regression_dispatch_decision(&plan, &detail.task) {
        RegressionDispatchDecision::Clear => {}
        RegressionDispatchDecision::Exempt { exemption: e, .. } => exemption = Some(e),
        RegressionDispatchDecision::Blocked { blocking } => {
            return Err(regression_blocked_error(blocking));
        }
    }
    // Same frontier policy for the review WIP gate: decide before any tab
    // exists. A task resuming after a bounce already holds its slot.
    if detail.task.status == crate::plan::TaskStatus::Pending {
        preflight_review_capacity(&plan, &detail.task).map_err(plan_rpc_err)?;
    }

    // Parent defaults to the dispatching (caller) tab, resolved server-side.
    let parent = owned_str(p, "parent_tab_id").or_else(|| owned_str(p, "caller_tab_id"));
    let parent_tab_id = parent
        .clone()
        .or_else(|| plan.owner.tab_id.clone())
        .ok_or_else(|| invalid_dispatch("dispatch requires a parent Orchestrator tab"))?;
    let routing = resolve_dispatch_routing(
        owned_str(p, "agent"),
        owned_str(p, "model"),
        detail.task.model.clone(),
        &svc.agent_registry.provider_ids(),
    );

    // Surface an unknown-model fallback loudly instead of silently downgrading.
    let resolved_provider = routing
        .agent
        .clone()
        .unwrap_or_else(crate::config::default_agent_provider);
    let mut warnings: Vec<String> = Vec::new();
    if let Some(notice) = crate::service::spawn::resolve_fresh_agent_model_with_notice(
        &svc.agent_registry,
        &resolved_provider,
        routing.model.clone(),
        None,
    )
    .fallback_notice
    {
        if let Some(error) = unqualified_model_error(&routing, &resolved_provider, &notice) {
            return Err(error);
        }
        warnings.push(format!(
            "requested model '{}' is not available for provider '{}'; spawning with '{}' instead",
            notice.requested_model, resolved_provider, notice.provider_resolved_model
        ));
    }

    // A personality is the executor persona: it names the tab and opens the brief.
    let (explicit_persona, title_provenance) = dispatch_worker_identity(&detail.task);
    let brief = build_brief(&detail.task, &detail.sections);

    let explicit_cwd = owned_str(p, "cwd");
    let canonical_cwd = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.clone()
    } else {
        plan.canonical_project_path.clone()
    };
    let lane_resolution =
        resolve_dispatch_lane(explicit_cwd.as_deref(), &canonical_cwd, &detail.task.lane)?;
    let mut spawn_params = SpawnAgentParams {
        agent: routing.agent.clone(),
        cwd: Some(lane_resolution.spawn_cwd.clone()),
        title: explicit_persona,
        title_provenance,
        role: Some("Worker".into()),
        parent_tab_id: parent,
        plan_id: Some(plan_id.clone()),
        task_id: Some(task_id.clone()),
        workspace_id: owned_str(p, "workspace_id"),
        model: routing.model.clone(),
        planned_model: detail.task.model.clone(),
        reasoning_effort: owned_str(p, "reasoning_effort"),
        spawned_by_tab_id: owned_str(p, "caller_tab_id"),
        task: Some(brief),
        ..Default::default()
    };

    let lane_label = format!("{plan_id}-{task_id}");
    let provisioned_lane = if lane_resolution.existing.is_some() {
        None
    } else {
        svc.provision_worker_lane_for_spawn(&mut spawn_params, &lane_label)
            .map_err(|error| dispatch_failed("lane_provision", error))?
    };
    let initial_provenance = match provisioned_lane.as_ref() {
        Some(lane) => lane.verify(),
        None => lane_resolution
            .existing
            .clone()
            .ok_or_else(|| "dispatch did not provision or resolve a worker lane".to_string()),
    };
    let initial_provenance = match initial_provenance {
        Ok(provenance) => provenance,
        Err(error) => {
            if let Some(lane) = provisioned_lane {
                if let Err(cleanup) = lane.cleanup() {
                    return Err(dispatch_failed(
                        "compensation",
                        format!("{error}; lane cleanup failed: {cleanup}"),
                    ));
                }
            }
            return Err(dispatch_failed("provenance_validation", error));
        }
    };
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let spawn = match svc.spawn_agent_sync_with_ready(spawn_params, Some(ready_tx)) {
        Ok(spawn) => spawn,
        Err(error) => {
            if let Some(lane) = provisioned_lane {
                if let Err(cleanup) = lane.cleanup() {
                    return Err(dispatch_failed(
                        "compensation",
                        format!("{error}; lane cleanup failed: {cleanup}"),
                    ));
                }
            }
            return Err(dispatch_failed("agent_spawn", error));
        }
    };
    let tab_id = spawn.tab_id.clone();
    let original_task = detail.task.clone();
    let original_plan_mode = plan.mode.clone();
    let lane_for_commit = detail.task.lane.clone();
    let starting_lane = lane_metadata_for_dispatch(&lane_for_commit, &initial_provenance);
    let attempt_id = match store.begin_task_dispatch(
        &plan_id,
        &task_id,
        &tab_id,
        &spawn.title,
        starting_lane,
        now,
    ) {
        Ok(attempt_id) => attempt_id,
        Err(error) => {
            let cleanup = rollback_failed_dispatch(
                provisioned_lane,
                || Ok(()),
                || close_tab_if_present(svc, &tab_id),
            );
            emit_plan_changed(app, &plan_id);
            return Err(dispatch_failed(
                "task_start",
                match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup) => format!("{error}; {cleanup}"),
                },
            ));
        }
    };
    emit_plan_changed(app, &plan_id);
    let startup = DispatchStartup {
        app: app.clone(),
        svc: svc.clone(),
        plan_id: plan_id.clone(),
        task_id: task_id.clone(),
        tab_id: tab_id.clone(),
        attempt_id: attempt_id.clone(),
        parent_tab_id,
        worker_title: spawn.title.clone(),
        original_task,
        original_plan_mode,
        lane_for_commit,
        provenance: initial_provenance.clone(),
        provisioned_lane,
    };
    std::thread::spawn(move || startup.run(ready_rx));

    let model_fallback = spawn.model_fallback.clone();
    let mut result = json!({
        "ok": true,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "agent": resolved_provider,
        "lane": {
            "worktree_path": initial_provenance.worktree_path,
            "branch": initial_provenance.branch,
            "base": initial_provenance.base,
            "canonical_project_path": initial_provenance.canonical_cwd,
            "destination": detail.task.lane.destination,
            "workspace_id": detail.task.lane.workspace_id,
            "workspace_name": detail.task.lane.workspace_name,
        },
    });
    apply_dispatch_starting_contract(&mut result, &tab_id, &spawn);
    if !warnings.is_empty() {
        result["warnings"] = json!(warnings);
    }
    if let Some(exemption) = exemption {
        result["regression_exception"] = json!({
            "exemption": exemption,
            "reason": exemption.reason(),
        });
    }
    if let Some(model_fallback) = model_fallback {
        result["model_fallback"] = json!(model_fallback);
    }
    Ok(result)
}

fn rollback_failed_dispatch<R, C>(
    lane: Option<ProvisionedWorkerLane>,
    revert_task: R,
    close_tab: C,
) -> Result<(), String>
where
    R: FnOnce() -> Result<(), String>,
    C: FnOnce() -> Result<(), String>,
{
    let mut errors = Vec::new();
    if let Err(error) = revert_task() {
        errors.push(format!("task restore failed: {error}"));
    }
    if let Err(error) = close_tab() {
        errors.push(format!("Tab close failed: {error}"));
    }
    if let Some(lane) = lane {
        if let Err(error) = lane.cleanup() {
            errors.push(format!("lane cleanup failed: {error}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Assemble the worker brief: body, acceptance, test plan, section links,
/// strides (milestones — T4 enriches later), and the mandatory report command.
pub(super) fn build_brief(task: &Task, sections: &[TaskSection]) -> String {
    build_brief_with_personality(task, sections, task.personality.as_deref())
}

pub(super) fn build_brief_with_personality(
    task: &Task,
    sections: &[TaskSection],
    effective_personality: Option<&str>,
) -> String {
    let mut out = String::new();
    if let Some(identity) = crate::service::plan::worker_identity::compose_worker_identity(
        effective_personality,
        task.role_guidance.as_deref(),
        Some(task.name.as_str()),
    ) {
        out.push_str(&identity);
        out.push_str("\n\n");
    }
    out.push_str(&format!("# {}\n\n", task.name));

    let body = task.body.trim();
    if !body.is_empty() {
        out.push_str(body);
        out.push_str("\n\n");
    }

    if !task.acceptance_criteria.is_empty() {
        out.push_str("## Acceptance criteria\n");
        for c in &task.acceptance_criteria {
            out.push_str(&format!("- {c}\n"));
        }
        out.push('\n');
    }

    if let Some(tp) = task
        .test_plan
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!("## Test plan\n{tp}\n\n"));
    }

    if !sections.is_empty() {
        out.push_str("## Sections\n");
        for s in sections {
            out.push_str(&format!("- {} ({})\n", s.title, s.id));
        }
        out.push('\n');
    }

    if !task.milestones.is_empty() {
        out.push_str("## Strides\n");
        for m in &task.milestones {
            out.push_str(&format!("- [{}] {}\n", m.kind, m.text));
        }
        out.push('\n');
    }

    out.push_str(
        "## When done\nRun: codeterm agent report \"STATUS: done | blocked | partial\\nCHANGES: ...\\nTESTS RUN: ...\"\n",
    );
    out
}

#[cfg(test)]
mod tests;
