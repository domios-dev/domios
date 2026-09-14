use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use crate::plan::TaskCompetitionMetadata;
use crate::service::{ServiceState, SpawnIdentityProvenance};

use super::plan::{emit_plan_changed, plan_rpc_err};
use super::plan_fields::tab_role;
use super::plan_refs;
use super::plan_role_helpers::record_spawned_attempt_reporting_close;
use super::{get_string_param, JsonRpcError};
use crate::multiplexer::types::TabCloseReason;

pub(super) fn metadata_field(
    p: Option<&Value>,
) -> Result<Option<TaskCompetitionMetadata>, JsonRpcError> {
    p.and_then(|x| x.get("competition"))
        .cloned()
        .map(|value| serde_json::from_value(value).map_err(invalid_metadata))
        .transpose()
}

pub(super) fn update_field(
    p: Option<&Value>,
) -> Result<Option<crate::service::plan::competition::TaskCompetitionUpdate>, JsonRpcError> {
    p.and_then(|x| x.get("competition"))
        .cloned()
        .map(|value| serde_json::from_value(value).map_err(invalid_update))
        .transpose()
}

fn invalid_metadata(e: serde_json::Error) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: format!("invalid competition metadata: {e}"),
        data: None,
    }
}

fn invalid_update(e: serde_json::Error) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: format!("invalid competition update: {e}"),
        data: None,
    }
}

fn preflight_compete_review_capacity(
    store: &crate::service::plan::PlanStore,
    plan_id: &str,
    task_id: &str,
) -> Result<(), crate::service::plan::PlanError> {
    let plan = store
        .get(plan_id)
        .ok_or(crate::service::plan::PlanError::NotFound)?;
    let task = plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or(crate::service::plan::PlanError::TaskNotFound)?;
    if task.status == crate::plan::TaskStatus::Pending {
        crate::service::plan::review_gate::validate_dispatch_review_capacity(&plan, task)?;
    }
    Ok(())
}

/// Only a Mutating task gets per-competitor lanes. Any other intent — including
/// the `Unspecified` default of a never-dispatched task — keeps today's path.
pub(super) fn mutating_competition(plan: &crate::plan::Plan, task_id: &str) -> bool {
    crate::plan::task_key::find_task(&plan.tasks, task_id).is_some_and(|task| {
        task.interruption.execution_intent == crate::plan::ExecutionIntent::Mutating
    })
}

fn lane_provision_failed(reason: String) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: format!("competition lane provisioning failed: {reason}"),
        data: Some(json!({
            "type": "competition_lane_provision_failed",
            "stage": "lane_provision",
            "reason": reason,
        })),
    }
}

/// Give this competitor its own worktree/branch/base off canonical, then prove
/// the result is neither canonical nor a lane another task owns — before spawn.
fn provision_competitor_lane(
    plan: &crate::plan::Plan,
    task_id: &str,
    draft: &mut crate::service::plan::task_attempts::TaskCompeteDraft,
) -> Result<crate::service::ProvisionedWorkerLane, JsonRpcError> {
    let canonical = super::plan_task_dispatch::lane_safety::canonical_path_of(plan).to_string();
    let label = format!("{}-{}-c{}", plan.id, task_id, draft.lane);
    let lane = crate::service::provision_isolated_lane(&canonical, &label)
        .map_err(lane_provision_failed)?;
    let provenance = match lane.verify() {
        Ok(provenance) => provenance,
        Err(error) => {
            return Err(release_unowned_lanes(
                vec![lane],
                lane_provision_failed(error),
            ))
        }
    };
    if let Err(refusal) = super::plan_task_dispatch::lane_safety::refuse_unsafe_mutating_write(
        plan,
        task_id,
        &provenance.worktree_path,
        crate::plan::ExecutionIntent::Mutating,
    ) {
        return Err(release_unowned_lanes(vec![lane], refusal));
    }
    draft.cwd = provenance.worktree_path;
    draft.branch = Some(provenance.branch);
    draft.base = Some(provenance.base);
    Ok(lane)
}

pub(super) struct SpawnedCompetitor {
    pub tab_id: String,
    pub attempt_id: String,
    pub lane: Option<crate::service::ProvisionedWorkerLane>,
}

/// Lanes with no tab and no recorded attempt behind them: safe to delete outright.
fn unowned_lanes(
    lanes: &mut [Option<crate::service::ProvisionedWorkerLane>],
    from: usize,
) -> Vec<crate::service::ProvisionedWorkerLane> {
    lanes[from..].iter_mut().filter_map(Option::take).collect()
}

fn release_unowned_lanes(
    lanes: Vec<crate::service::ProvisionedWorkerLane>,
    error: JsonRpcError,
) -> JsonRpcError {
    let failures: Vec<String> = lanes
        .into_iter()
        .filter_map(|lane| lane.cleanup().err())
        .collect();
    if failures.is_empty() {
        return error;
    }
    JsonRpcError {
        code: error.code,
        message: format!(
            "{}; lane cleanup failed: {}",
            error.message,
            failures.join("; ")
        ),
        data: error.data,
    }
}

/// Phase 1: every mutating lane is provisioned and verified before any spawn, so
/// a provisioning failure rolls back with zero tabs and zero attempts in play.
type CompetitorLanes = Vec<Option<crate::service::ProvisionedWorkerLane>>;

fn provision_all_competitor_lanes(
    plan: &crate::plan::Plan,
    task_id: &str,
    mut drafts: Vec<crate::service::plan::task_attempts::TaskCompeteDraft>,
) -> Result<
    (
        Vec<crate::service::plan::task_attempts::TaskCompeteDraft>,
        CompetitorLanes,
    ),
    JsonRpcError,
> {
    let mut lanes: CompetitorLanes = std::iter::repeat_with(|| None).take(drafts.len()).collect();
    if !mutating_competition(plan, task_id) {
        return Ok((drafts, lanes));
    }
    for index in 0..drafts.len() {
        match provision_competitor_lane(plan, task_id, &mut drafts[index]) {
            Ok(lane) => lanes[index] = Some(lane),
            Err(error) => {
                let provisioned = unowned_lanes(&mut lanes, 0);
                return Err(release_unowned_lanes(provisioned, error));
            }
        }
    }
    Ok((drafts, lanes))
}

pub(super) struct CompetitorCompensation {
    pub tab_id: String,
    pub attempt_id: String,
    pub worktree_path: Option<String>,
}

#[derive(Default)]
pub(super) struct CompensationOutcome {
    pub released: Vec<Value>,
    pub retained: Vec<Value>,
}

impl CompensationOutcome {
    fn is_empty(&self) -> bool {
        self.released.is_empty() && self.retained.is_empty()
    }
}

/// Ownership order per competitor: stop the tab, retire its attempt, and ONLY
/// then delete its lane; a failed step retains the worktree instead of deleting.
pub(super) fn compensate_spawned_competitors<C, R, L>(
    competitors: Vec<CompetitorCompensation>,
    mut close_tab: C,
    mut retire_attempt: R,
    mut cleanup_lane: L,
) -> CompensationOutcome
where
    C: FnMut(&str) -> Result<(), String>,
    R: FnMut(&str) -> Result<(), String>,
    L: FnMut(&str) -> Result<(), String>,
{
    let mut outcome = CompensationOutcome::default();
    for competitor in competitors {
        let blocker = close_tab(&competitor.tab_id)
            .err()
            .or_else(|| retire_attempt(&competitor.tab_id).err());
        if let Some(reason) = blocker {
            outcome.retained.push(json!({
                "tab_id": competitor.tab_id,
                "attempt_id": competitor.attempt_id,
                "worktree_path": competitor.worktree_path,
                "reason": reason,
            }));
            continue;
        }
        match competitor.worktree_path.as_deref().map(&mut cleanup_lane) {
            Some(Err(reason)) => outcome.retained.push(json!({
                "tab_id": competitor.tab_id,
                "attempt_id": competitor.attempt_id,
                "worktree_path": competitor.worktree_path,
                "reason": reason,
            })),
            _ => outcome.released.push(json!({
                "tab_id": competitor.tab_id,
                "attempt_id": competitor.attempt_id,
                "worktree_path": competitor.worktree_path,
            })),
        }
    }
    outcome
}

/// Owns a tab but NO attempt, so retirement is never attempted for it; its lane
/// is released only once its tab is confirmed closed.
pub(super) fn compensate_current_competitor<L>(
    tab_id: &str,
    worktree_path: Option<&str>,
    close_error: Option<String>,
    mut cleanup_lane: L,
) -> CompensationOutcome
where
    L: FnMut(&str) -> Result<(), String>,
{
    let mut outcome = CompensationOutcome::default();
    if let Some(reason) = close_error {
        outcome.retained.push(json!({
            "tab_id": tab_id,
            "attempt_id": Value::Null,
            "worktree_path": worktree_path,
            "reason": reason,
        }));
        return outcome;
    }
    match worktree_path.map(&mut cleanup_lane) {
        Some(Err(reason)) => outcome.retained.push(json!({
            "tab_id": tab_id,
            "attempt_id": Value::Null,
            "worktree_path": worktree_path,
            "reason": reason,
        })),
        _ => outcome.released.push(json!({
            "tab_id": tab_id,
            "attempt_id": Value::Null,
            "worktree_path": worktree_path,
        })),
    }
    outcome
}

/// A spawned competitor whose attempt was never recorded: it is compensated
/// separately because there is no attempt to retire.
pub(super) struct UnrecordedCompetitor {
    pub tab_id: String,
    pub lane: Option<crate::service::ProvisionedWorkerLane>,
    pub close_error: Option<String>,
}

fn compensate_competition(
    svc: &ServiceState,
    plan_id: &str,
    task_id: &str,
    live: Vec<SpawnedCompetitor>,
    current: Option<UnrecordedCompetitor>,
    unowned: Vec<crate::service::ProvisionedWorkerLane>,
    now: u64,
    error: JsonRpcError,
) -> JsonRpcError {
    let mut owned_lanes: std::collections::HashMap<String, crate::service::ProvisionedWorkerLane> =
        std::collections::HashMap::new();
    let competitors = live
        .into_iter()
        .map(|competitor| CompetitorCompensation {
            tab_id: competitor.tab_id,
            attempt_id: competitor.attempt_id,
            worktree_path: competitor.lane.map(|lane| {
                let path = lane.cwd.clone();
                owned_lanes.insert(path.clone(), lane);
                path
            }),
        })
        .collect();
    let current = current.map(|current| {
        let path = current.lane.map(|lane| {
            let path = lane.cwd.clone();
            owned_lanes.insert(path.clone(), lane);
            path
        });
        (current.tab_id, current.close_error, path)
    });
    let mut outcome = compensate_spawned_competitors(
        competitors,
        |tab_id| svc.close_tab_with_reason(tab_id, TabCloseReason::Cancelled),
        |tab_id| {
            svc.plan_store
                .flag_tab_gone(plan_id, task_id, tab_id, now)
                .map(|_| ())
                .map_err(|error| error.to_string())
        },
        |path| match owned_lanes.remove(path) {
            Some(lane) => lane.cleanup(),
            None => Ok(()),
        },
    );
    if let Some((tab_id, close_error, path)) = current {
        let current_outcome =
            compensate_current_competitor(&tab_id, path.as_deref(), close_error, |path| {
                match owned_lanes.remove(path) {
                    Some(lane) => lane.cleanup(),
                    None => Ok(()),
                }
            });
        outcome.released.extend(current_outcome.released);
        outcome.retained.extend(current_outcome.retained);
    }
    let mut error = release_unowned_lanes(unowned, error);
    if outcome.is_empty() {
        return error;
    }
    let partial_state = json!({
        "released": outcome.released,
        "retained": outcome.retained,
    });
    match error.data.as_mut() {
        Some(Value::Object(data)) => {
            data.insert("partial_state".into(), partial_state);
        }
        _ => error.data = Some(json!({ "partial_state": partial_state })),
    }
    error
}

pub(super) fn compete(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let p = params.as_ref();
    let (id, tid) = plan_refs::resolve_task(store, params)?;
    super::plan_task_rpc::authorize_execution(svc, &id, p)?;
    preflight_compete_review_capacity(store, &id, &tid).map_err(plan_rpc_err)?;
    let body = p.ok_or(JsonRpcError {
        code: -32602,
        message: "params required".to_string(),
        data: None,
    })?;
    let request = crate::service::plan::task_attempts::TaskCompeteRequest {
        attempts: body
            .get("attempts")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        provider: body
            .get("provider")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        models: body
            .get("models")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        parent_tab_id: body
            .get("parent_tab_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        workspace_id: body
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    };
    let drafts = store
        .build_task_compete_drafts(&id, &tid, request)
        .map_err(plan_rpc_err)?;
    let plan = store
        .get(&id)
        .ok_or_else(|| plan_rpc_err(crate::service::plan::PlanError::NotFound))?;
    // Phase 1 — provision and verify EVERY mutating lane before the first spawn,
    // so a provisioning failure can never delete underneath a running competitor.
    let (mut drafts, mut lanes) = provision_all_competitor_lanes(&plan, &tid, drafts)?;
    let mut attempts = Vec::new();
    let mut spawns = Vec::new();
    let mut live: Vec<SpawnedCompetitor> = Vec::new();
    // Phase 2 — spawn. From here compensation runs in ownership order.
    for index in 0..drafts.len() {
        let draft = &mut drafts[index];
        let parent_tab_id = match draft
            .parent_tab_id
            .clone()
            .or_else(|| plan.owner.tab_id.clone())
        {
            Some(parent_tab_id) => parent_tab_id,
            None => {
                return Err(compensate_competition(
                    svc,
                    &id,
                    &tid,
                    live,
                    None,
                    unowned_lanes(&mut lanes, index),
                    now,
                    JsonRpcError {
                        code: -32602,
                        message: "competition requires a parent Orchestrator tab".into(),
                        data: None,
                    },
                ))
            }
        };
        let spawn_params = crate::service::SpawnAgentParams {
            agent: Some(draft.provider.clone()),
            cwd: Some(draft.cwd.clone()),
            title: draft.title.clone(),
            title_provenance: Some(SpawnIdentityProvenance::Suggested),
            role: Some("Worker".into()),
            parent_tab_id: Some(parent_tab_id.clone()),
            task: Some(draft.task_prompt.clone()),
            workspace_id: draft.workspace_id.clone(),
            model: draft.model.clone(),
            planned_model: draft.planned_model.clone(),
            spawned_by_tab_id: Some(parent_tab_id.clone()),
            provisioned_cwd: draft.branch.is_some().then(|| draft.cwd.clone()),
            ..Default::default()
        };
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        // A failed spawn creates no tab, and record_spawned_attempt closes the tab
        // it spawned, so THIS competitor's lane is unowned on either failure.
        let spawn = match svc.spawn_agent_sync_with_ready(spawn_params, Some(ready_tx)) {
            Ok(spawn) => spawn,
            Err(e) => {
                return Err(compensate_competition(
                    svc,
                    &id,
                    &tid,
                    live,
                    None,
                    unowned_lanes(&mut lanes, index),
                    now,
                    JsonRpcError {
                        code: -32603,
                        message: e,
                        data: None,
                    },
                ))
            }
        };
        let created = match record_spawned_attempt_reporting_close(svc, &spawn.tab_id, || {
            store.record_task_compete_attempt(
                &id,
                &tid,
                &spawn.tab_id,
                draft,
                tab_role(svc, &spawn.tab_id),
                now,
            )
        }) {
            Ok(created) => created,
            Err(failure) => {
                // No attempt was recorded, so nothing to retire; the lane is only
                // safe to delete once this tab is CONFIRMED closed.
                let current = UnrecordedCompetitor {
                    tab_id: spawn.tab_id.clone(),
                    lane: lanes[index].take(),
                    close_error: failure.close_error,
                };
                return Err(compensate_competition(
                    svc,
                    &id,
                    &tid,
                    live,
                    Some(current),
                    unowned_lanes(&mut lanes, index),
                    now,
                    failure.error,
                ));
            }
        };
        live.push(SpawnedCompetitor {
            tab_id: spawn.tab_id.clone(),
            attempt_id: created.attempt.id.clone(),
            lane: lanes[index].take(),
        });
        let svc_for_ready = svc.clone();
        let app_for_ready = app.clone();
        let plan_id = id.clone();
        let task_id = tid.clone();
        let tab_id = spawn.tab_id.clone();
        let attempt_id = created.attempt.id.clone();
        let worker_title = spawn.title.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(60);
            let runtime_ready = super::plan_task_dispatch::await_agent_ready(ready_rx, deadline)
                .and_then(|_| {
                    super::plan_task_dispatch::wait_for_tab_session(
                        &svc_for_ready,
                        &tab_id,
                        deadline,
                    )
                });
            match runtime_ready {
                Ok(session_id) => {
                    if let Err(error) = svc_for_ready.plan_store.mark_task_attempt_runtime_ready(
                        &plan_id,
                        &task_id,
                        &tab_id,
                        &attempt_id,
                        &session_id,
                        epoch_ms(),
                    ) {
                        super::plan_task_dispatch::publish_startup_report(
                            &svc_for_ready,
                            &parent_tab_id,
                            &tab_id,
                            &worker_title,
                            &plan_id,
                            &task_id,
                            json!({
                                "kind": "dispatch_startup_failed",
                                "stage": "task_commit",
                                "error": error.to_string(),
                            }),
                        );
                        return;
                    }
                    super::plan_task_dispatch::arm_worker_ack_deadline(
                        &svc_for_ready,
                        &parent_tab_id,
                        &tab_id,
                        &worker_title,
                        &plan_id,
                        &task_id,
                        &attempt_id,
                        format!("codeterm plan task compete {plan_id} {task_id}"),
                        std::time::Duration::ZERO,
                    );
                    emit_plan_changed(&app_for_ready, &plan_id);
                    super::plan_task_dispatch::publish_startup_report(
                        &svc_for_ready,
                        &parent_tab_id,
                        &tab_id,
                        &worker_title,
                        &plan_id,
                        &task_id,
                        json!({
                            "kind": "dispatch_ready",
                            "plan_id": plan_id,
                            "task_id": task_id,
                            "tab_id": tab_id,
                            "attempt_id": attempt_id,
                            "session_id": session_id,
                        }),
                    );
                }
                Err(error) => {
                    let _ =
                        svc_for_ready.close_tab_with_reason(&tab_id, TabCloseReason::SpawnRollback);
                    emit_plan_changed(&app_for_ready, &plan_id);
                    super::plan_task_dispatch::publish_startup_report(
                        &svc_for_ready,
                        &parent_tab_id,
                        &tab_id,
                        &worker_title,
                        &plan_id,
                        &task_id,
                        json!({
                            "kind": "dispatch_startup_failed",
                            "stage": "agent_ready",
                            "error": error.message(),
                        }),
                    );
                }
            }
        });
        attempts.push(created.attempt);
        spawns.push(spawn);
    }
    emit_plan_changed(app, &id);
    let group = attempts
        .first()
        .and_then(|attempt| attempt.competition_group_id.clone())
        .unwrap_or_default();
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
        "competition_group_id": group,
        "attempts": attempts,
        "spawns": spawns
    }))
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod lane_isolation_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{AgentRef, OrchestratorRef, Plan, ReviewCapacityClass};
    use crate::service::plan::{NewTask, PlanStore};

    #[test]
    fn compete_allows_review_debt_without_capacity_grants() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "competition".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "spoofed review".into(),
                    kind: "review".into(),
                    tags: vec!["review-capacity".into(), "canonical-gate".into()],
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let mut persisted = store.get(&plan.id).unwrap();
        persisted.review_policy.credit = -(persisted.review_policy.wip_limit as i64);
        store.save(&mut persisted, 3).unwrap();

        preflight_compete_review_capacity(&store, &plan.id, &task_id).unwrap();
        store
            .grant_review_capacity(
                &plan.id,
                &task_id,
                ReviewCapacityClass::Review,
                AgentRef::from_tab("orch"),
                4,
            )
            .unwrap();
        preflight_compete_review_capacity(&store, &plan.id, &task_id).unwrap();
    }
}

pub(super) fn judge(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let p = params.as_ref();
    let (id, tid) = plan_refs::resolve_task(store, params)?;
    let attempt_id = get_string_param(params, "attempt_id")?;
    let verdict = get_string_param(params, "verdict")?;
    let reason = p
        .and_then(|x| x.get("reason"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty());
    let winner = p.and_then(|x| x.get("winner")).and_then(|v| v.as_bool()) == Some(true);
    let judged = store
        .judge_task_attempt(&id, &tid, &attempt_id, &verdict, reason, winner, now)
        .map_err(plan_rpc_err)?;
    emit_plan_changed(app, &id);
    Ok(json!({
        "ok": true,
        "plan_id": id,
        "task_id": tid,
        "attempt": judged.attempt
    }))
}
