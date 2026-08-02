use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use crate::plan::{Task, TaskLaneMetadata};
use crate::service::plan::milestone_gates::{
    regression_dispatch_decision, DispatchExemption, RegressionDispatchDecision, RegressionGate,
};
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

fn dispatch_reasoning_effort(p: Option<&Value>) -> (Option<String>, Option<String>) {
    let requested = owned_str(p, "reasoning_effort");
    (requested.clone(), requested)
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

fn verify_dispatched_task_binding(
    store: &crate::service::plan::PlanStore,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
) -> Result<(), String> {
    match store.task_binding_for_tab(tab_id) {
        Some((bound_plan, bound_task)) if bound_plan == plan_id && bound_task == task_id => Ok(()),
        binding => Err(format!(
            "ORCHESTRATION PRODUCT INCIDENT: dispatched Worker tab '{tab_id}' has no matching \
             authoritative task binding. Impact: zero-flag reports cannot be attached safely. \
             Fallback: pass explicit --plan {plan_id} --task {task_id} and alert the Orchestrator; \
             no plan or task was guessed. Resolved binding: {binding:?}"
        )),
    }
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

fn preflight_checkout_occupancy(
    plan: &crate::plan::Plan,
    dispatched_task_id: &str,
    checkout_path: &str,
) -> Result<(), JsonRpcError> {
    let canonical_path = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.as_str()
    } else {
        plan.canonical_project_path.as_str()
    };
    let normalized_checkout = codeterm_paths::normalize_cwd(checkout_path);
    let occupant = plan.tasks.iter().find_map(|task| {
        task.execution_attempts.iter().find_map(|attempt| {
            if attempt.closed_at.is_some() {
                return None;
            }
            let attempt_path = attempt
                .worktree_path
                .as_deref()
                .filter(|path| !path.trim().is_empty())
                .unwrap_or(canonical_path);
            (normalized_checkout == codeterm_paths::normalize_cwd(attempt_path))
                .then_some((task.id.as_str(), attempt.id.as_str()))
        })
    });
    let Some((occupying_task_id, occupying_attempt_id)) = occupant else {
        return Ok(());
    };
    let update_command = isolated_lane_update_command(&plan.id, dispatched_task_id);
    let dispatch_command = format!(
        "codeterm plan task dispatch {} {}",
        plan.id, dispatched_task_id
    );
    let message = format!(
        "dispatch refused: checkout '{checkout_path}' is occupied by live attempt \
         '{occupying_attempt_id}' of task '{occupying_task_id}' in plan '{}'. No worker was \
         spawned. Assign an existing isolated lane, then dispatch again:\n  {update_command}\n  \
         {dispatch_command}",
        plan.id
    );
    Err(JsonRpcError {
        code: -32011,
        message: message.clone(),
        data: Some(json!({
            "type": "checkout_occupied",
            "stage": "checkout_occupancy",
            "reason": message,
            "occupying_task_id": occupying_task_id,
            "occupying_attempt_id": occupying_attempt_id,
            "checkout_path": checkout_path,
            "update_command": update_command,
            "dispatch_command": dispatch_command,
        })),
    })
}

#[cfg(windows)]
fn isolated_lane_update_command(plan_id: &str, task_id: &str) -> String {
    format!(
        "codeterm plan task update {plan_id} {task_id} --worktree (Read-Host 'Existing isolated worktree path') --branch (Read-Host 'Existing isolated branch') --base (Read-Host 'Base ref')"
    )
}

#[cfg(not(windows))]
fn isolated_lane_update_command(plan_id: &str, task_id: &str) -> String {
    format!(
        "codeterm plan task update {plan_id} {task_id} \
         --worktree \"$(printf 'Existing isolated worktree path: ' >/dev/tty; IFS= read -r value </dev/tty; printf '%s' \"$value\")\" \
         --branch \"$(printf 'Existing isolated branch: ' >/dev/tty; IFS= read -r value </dev/tty; printf '%s' \"$value\")\" \
         --base \"$(printf 'Base ref: ' >/dev/tty; IFS= read -r value </dev/tty; printf '%s' \"$value\")\""
    )
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

fn task_launch_authority_for_dispatch(
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    lane: &TaskLaneMetadata,
) -> crate::service::TaskLaunchAuthority {
    crate::service::TaskLaunchAuthority {
        plan_id: plan_id.to_string(),
        task_id: task_id.to_string(),
        attempt_id: attempt_id.to_string(),
        lane_base: lane.base.clone(),
    }
}

pub(super) fn await_agent_ready(
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

pub(super) fn wait_for_tab_session(
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
    lane: Option<WorkerLaneProvenance>,
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
    let mut event = json!({
        "kind": "dispatch_ready",
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
        "session_id": evidence.session_id,
    });
    if let Some(lane) = evidence.lane.as_ref() {
        event["lane"] = json!({
            "worktree_path": lane.worktree_path,
            "branch": lane.branch,
            "base": lane.base,
            "canonical_project_path": lane.canonical_cwd,
        });
    }
    event
}

fn checkout_required(reason: impl Into<String>) -> JsonRpcError {
    let reason = reason.into();
    JsonRpcError {
        code: -32012,
        message: format!("repository checkout required: {reason}"),
        data: Some(json!({
            "type": "checkout_required",
            "stage": "checkout_preflight",
            "reason": reason,
        })),
    }
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn publish_startup_report(
    svc: &ServiceState,
    parent_tab_id: &str,
    worker_tab_id: &str,
    worker_title: &str,
    plan_id: &str,
    task_id: &str,
    payload: Value,
) {
    svc.publish_worker_lifecycle_report(
        parent_tab_id,
        worker_tab_id,
        worker_title,
        plan_id,
        task_id,
        payload,
    );
}

pub(super) fn arm_worker_ack_deadline(
    svc: &ServiceState,
    parent_tab_id: &str,
    worker_tab_id: &str,
    worker_title: &str,
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    spawn_command: String,
) {
    svc.arm_worker_ack_deadline(
        parent_tab_id,
        worker_tab_id,
        worker_title,
        plan_id,
        task_id,
        attempt_id,
        spawn_command,
    );
}

#[cfg(test)]
fn worker_ack_timeout_event(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt_id: &str,
    spawn_command: &str,
) -> Value {
    crate::service::worker_ack::worker_ack_timeout_event(
        plan_id,
        task_id,
        tab_id,
        attempt_id,
        spawn_command,
        None,
    )
}

pub(super) struct DispatchStartup {
    pub(super) app: AppHandle,
    pub(super) svc: ServiceState,
    pub(super) plan_id: String,
    pub(super) task_id: String,
    pub(super) tab_id: String,
    pub(super) attempt_id: String,
    pub(super) parent_tab_id: String,
    pub(super) worker_title: String,
    pub(super) original_task: Task,
    pub(super) original_plan_mode: crate::plan::PlanMode,
    pub(super) lane_for_commit: TaskLaneMetadata,
    pub(super) provenance: Option<WorkerLaneProvenance>,
    pub(super) provisioned_lane: Option<ProvisionedWorkerLane>,
    pub(super) spawn_command: String,
}

impl DispatchStartup {
    pub(super) fn run(self, ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>) {
        let deadline = Instant::now() + DISPATCH_READY_TIMEOUT;
        let provisioned_lane = self.provisioned_lane;
        let outcome = complete_dispatch_transaction(
            deadline,
            |deadline| await_agent_ready(ready_rx, deadline),
            |deadline| wait_for_tab_session(&self.svc, &self.tab_id, deadline),
            || match self.provenance.as_ref() {
                Some(provenance) => verify_worker_lane(
                    &provenance.canonical_cwd,
                    &provenance.worktree_path,
                    Some(&provenance.branch),
                    Some(&provenance.base),
                )
                .map(Some),
                None => Ok(None),
            },
            |lane, session_id| {
                let session_binding_nonce = self
                    .svc
                    .mux
                    .read()
                    .get_tab(&self.tab_id)
                    .and_then(|tab| tab.session_binding_nonce.clone())
                    .filter(|nonce| !nonce.trim().is_empty())
                    .ok_or_else(|| "ready Worker tab has no session binding nonce".to_string())?;
                let lane_metadata = lane
                    .as_ref()
                    .map(|lane| lane_metadata_for_dispatch(&self.lane_for_commit, lane))
                    .unwrap_or_else(|| {
                        let mut metadata = self.lane_for_commit.clone();
                        metadata.worktree_path = None;
                        metadata.branch = None;
                        metadata.base = None;
                        metadata.cleanup_status = None;
                        metadata
                    });
                self.svc
                    .plan_store
                    .finish_task_dispatch(
                        &self.plan_id,
                        &self.task_id,
                        &self.tab_id,
                        &self.attempt_id,
                        session_id,
                        &session_binding_nonce,
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
                arm_worker_ack_deadline(
                    &self.svc,
                    &self.parent_tab_id,
                    &self.tab_id,
                    &self.worker_title,
                    &self.plan_id,
                    &self.task_id,
                    &self.attempt_id,
                    self.spawn_command.clone(),
                );
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
    Provenance: FnOnce() -> Result<Option<WorkerLaneProvenance>, String>,
    Commit: FnOnce(&Option<WorkerLaneProvenance>, &str) -> Result<(), String>,
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

/// Spawn a Worker and record runtime readiness without claiming ownership.
/// Failure before ACK restores plan state and closes the spawned tab.
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
    svc.authorize_execution_action(
        &plan_id,
        &crate::service::AuthenticatedPlanCaller {
            tab_id: owned_str(p, "caller_tab_id"),
            session_id: owned_str(p, "session_id"),
            binding_nonce: owned_str(p, "session_binding_nonce"),
        },
    )
    .map_err(|message| JsonRpcError {
        code: -32003,
        message,
        data: Some(json!({ "type": "plan_orchestrator_authorization_required" })),
    })?;
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
    let execution_intent = match owned_str(p, "execution_intent").as_deref() {
        Some("read_only") => crate::plan::ExecutionIntent::ReadOnly,
        Some("mutating") => crate::plan::ExecutionIntent::Mutating,
        _ => {
            return Err(invalid_dispatch(
                "dispatch requires execution_intent read_only or mutating",
            ))
        }
    };
    let allowance_capability = svc
        .agent_registry
        .with_provider(&resolved_provider, |provider| {
            provider.provider_allowance_capability()
        })
        .flatten();
    let reserve_pct = p
        .and_then(|value| value.get("provider_reserve_pct"))
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .or_else(|| allowance_capability.map(|capability| capability.default_reserve_pct))
        .unwrap_or(5.0);
    let allowance = if execution_intent == crate::plan::ExecutionIntent::Mutating {
        let allowance_capability = allowance_capability.ok_or_else(|| {
            invalid_dispatch("selected provider has no mutation allowance capability")
        })?;
        let snapshot = svc
            .usage_store
            .get_latest(&resolved_provider, None)
            .map_err(|error| invalid_dispatch(format!("provider allowance unavailable: {error}")))?
            .ok_or_else(|| invalid_dispatch("provider allowance telemetry is unknown"))?;
        Some(crate::plan::ProviderAllowance {
            remaining_pct: crate::usage::allowance_remaining_pct(&snapshot),
            observed_at: snapshot.captured_at.max(0) as u64,
            stale_after_ms: allowance_capability.telemetry_max_age_ms,
            reserve_pct,
        })
    } else {
        None
    };
    crate::service::plan::interruption::provider_allowance_preflight(
        &execution_intent,
        allowance.as_ref(),
        now,
    )
    .map_err(plan_rpc_err)?;
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
    let manifest = crate::service::plan::plan_skills::plan_skill_manifest(store, &plan_id)
        .map_err(plan_rpc_err)?;
    let brief = build_brief_with_manifest(&detail.task, &detail.sections, &plan_id, &manifest);

    let explicit_cwd = owned_str(p, "cwd");
    let canonical_cwd = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.clone()
    } else {
        plan.canonical_project_path.clone()
    };
    let requires_repository = detail.task.execution_intent.requires_repository();
    let lane_resolution = if requires_repository {
        verify_worker_lane(&canonical_cwd, &canonical_cwd, None, None)
            .map_err(checkout_required)?;
        Some(resolve_dispatch_lane(
            explicit_cwd.as_deref(),
            &canonical_cwd,
            &detail.task.lane,
        )?)
    } else {
        None
    };
    let spawn_cwd = lane_resolution
        .as_ref()
        .map(|lane| lane.spawn_cwd.clone())
        .unwrap_or_else(|| {
            explicit_cwd
                .clone()
                .unwrap_or_else(|| canonical_cwd.clone())
        });
    if requires_repository {
        preflight_checkout_occupancy(&plan, &task_id, &spawn_cwd)?;
    }
    let (planned_reasoning_effort, reasoning_effort) = dispatch_reasoning_effort(p);
    let mut spawn_params = SpawnAgentParams {
        agent: routing.agent.clone(),
        cwd: Some(spawn_cwd),
        title: explicit_persona,
        title_provenance,
        role: Some("Worker".into()),
        parent_tab_id: parent,
        plan_id: Some(plan_id.clone()),
        task_id: Some(task_id.clone()),
        workspace_id: owned_str(p, "workspace_id"),
        model: routing.model.clone(),
        planned_model: detail.task.model.clone(),
        planned_reasoning_effort,
        reasoning_effort,
        spawned_by_tab_id: owned_str(p, "caller_tab_id"),
        task: Some(brief),
        ..Default::default()
    };

    let lane_label = format!("{plan_id}-{task_id}");
    let provisioned_lane = if requires_repository
        && lane_resolution
            .as_ref()
            .is_some_and(|resolution| resolution.existing.is_none())
    {
        svc.provision_worker_lane_for_spawn(&mut spawn_params, &lane_label)
            .map_err(|error| dispatch_failed("lane_provision", error))?
    } else {
        None
    };
    let initial_provenance = if requires_repository {
        match provisioned_lane.as_ref() {
            Some(lane) => lane.verify(),
            None => lane_resolution
                .as_ref()
                .and_then(|resolution| resolution.existing.clone())
                .ok_or_else(|| "dispatch did not provision or resolve a worker lane".to_string()),
        }
        .map(Some)
    } else {
        Ok(None)
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
    let original_task = detail.task.clone();
    let original_plan_mode = plan.mode.clone();
    let lane_for_commit = detail.task.lane.clone();
    let starting_lane = initial_provenance
        .as_ref()
        .map(|provenance| lane_metadata_for_dispatch(&lane_for_commit, provenance))
        .unwrap_or_else(|| {
            let mut lane = lane_for_commit.clone();
            lane.worktree_path = None;
            lane.branch = None;
            lane.base = None;
            lane.cleanup_status = None;
            lane
        });
    let reserved_attempt_id = crate::plan::ids::short_id("attempt");
    spawn_params.task_launch_authority = Some(task_launch_authority_for_dispatch(
        &plan_id,
        &task_id,
        &reserved_attempt_id,
        &starting_lane,
    ));
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
    let attempt_id = match store.begin_task_dispatch_with_reserved_attempt(
        &plan_id,
        &task_id,
        &tab_id,
        &spawn.title,
        starting_lane,
        execution_intent,
        allowance,
        Some(&resolved_provider),
        &reserved_attempt_id,
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
    if let Err(error) =
        store.set_attempt_plan_skill_manifest(&plan_id, &task_id, &attempt_id, manifest, now)
    {
        let cleanup = rollback_failed_dispatch(
            provisioned_lane,
            || {
                store
                    .revert_task_dispatch(&plan_id, &task_id, now)
                    .map_err(|error| error.to_string())
            },
            || close_tab_if_present(svc, &tab_id),
        );
        return Err(dispatch_failed(
            "skill_manifest",
            match cleanup {
                Ok(()) => error.to_string(),
                Err(cleanup) => format!("{error}; {cleanup}"),
            },
        ));
    }
    if let Err(error) = verify_dispatched_task_binding(store, &plan_id, &task_id, &tab_id) {
        let cleanup = rollback_failed_dispatch(
            provisioned_lane,
            || {
                store
                    .restore_task_dispatch_snapshot(
                        &plan_id,
                        &original_task,
                        original_plan_mode.clone(),
                        now,
                    )
                    .map_err(|error| error.to_string())
            },
            || close_tab_if_present(svc, &tab_id),
        );
        emit_plan_changed(app, &plan_id);
        return Err(dispatch_failed(
            "task_binding",
            match cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; {cleanup}"),
            },
        ));
    }
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
        spawn_command: format!("codeterm plan task dispatch {plan_id} {task_id}"),
    };
    std::thread::spawn(move || startup.run(ready_rx));

    let model_fallback = spawn.model_fallback.clone();
    let mut result = json!({
        "ok": true,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "agent": resolved_provider,
    });
    if let Some(provenance) = initial_provenance.as_ref() {
        result["lane"] = json!({
            "worktree_path": provenance.worktree_path,
            "branch": provenance.branch,
            "base": provenance.base,
            "canonical_project_path": provenance.canonical_cwd,
            "destination": detail.task.lane.destination,
            "workspace_id": detail.task.lane.workspace_id,
            "workspace_name": detail.task.lane.workspace_name,
        });
    }
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
pub(super) fn build_brief(
    task: &Task,
    sections: &[crate::service::plan::task_queries::TaskSection],
) -> String {
    build_brief_with_personality(task, sections, task.personality.as_deref())
}

pub(super) fn build_brief_with_personality(
    task: &Task,
    sections: &[crate::service::plan::task_queries::TaskSection],
    effective_personality: Option<&str>,
) -> String {
    crate::service::plan::task_brief::render_task_brief(
        task,
        sections,
        effective_personality,
        None,
        None,
        &[],
    )
}

fn build_brief_with_manifest(
    task: &Task,
    sections: &[crate::service::plan::task_queries::TaskSection],
    plan_id: &str,
    manifest: &[crate::plan::PlanSkillManifestEntry],
) -> String {
    crate::service::plan::task_brief::render_task_brief(
        task,
        sections,
        task.personality.as_deref(),
        None,
        Some(plan_id),
        manifest,
    )
}

#[cfg(test)]
mod tests;
