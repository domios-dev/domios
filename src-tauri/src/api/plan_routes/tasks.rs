use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::api::routes::{check_auth, AppState};
use crate::multiplexer::commands::MuxState;
use crate::plan::{
    AgentRef, IdentityProvenance, OrchestratorRef, TaskCompetitionMetadata, TaskLaneMetadata,
};
use crate::service::plan::task_attempts::TaskRespawnRequest;
use crate::service::plan::{NewTask, TaskLaneUpdate};
use crate::service::{persist_attempt_executor_identity_for_mux, respawn_title_provenance};

use super::{
    emit_plan_changed, epoch_ms, optional_string_field, plan_err, text_err, PlanRouteResult,
};

fn planner_role_for_tab(state: &AppState, tab_id: &str) -> Option<&'static str> {
    state.svc.mux.read().get_tab(tab_id).and_then(|tab| {
        (tab.role == Some(crate::multiplexer::types::AgentRole::Manager)
            && tab.role_profile.as_deref() == Some("planner"))
        .then_some("planner")
    })
}

fn record_respawn_attempt_and_persist_identity(
    state: &AppState,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    draft: &crate::service::plan::task_attempts::TaskRespawnDraft,
    title: &str,
    now_ms: u64,
) -> Result<crate::service::plan::task_attempts::TaskAttemptCreated, crate::service::plan::PlanError>
{
    record_respawn_attempt_and_persist_identity_for(
        &state.svc.plan_store,
        &state.svc.mux,
        planner_role_for_tab(state, tab_id).or(Some("worker")),
        plan_id,
        task_id,
        tab_id,
        draft,
        title,
        now_ms,
    )
}

fn record_respawn_attempt_and_persist_identity_for(
    store: &crate::service::plan::PlanStore,
    mux: &MuxState,
    executor_role: Option<&str>,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    draft: &crate::service::plan::task_attempts::TaskRespawnDraft,
    title: &str,
    now_ms: u64,
) -> Result<crate::service::plan::task_attempts::TaskAttemptCreated, crate::service::plan::PlanError>
{
    let created = store.record_task_respawn_attempt(
        plan_id,
        task_id,
        tab_id,
        draft,
        executor_role,
        now_ms,
    )?;
    persist_attempt_executor_identity_for_mux(store, mux, plan_id, task_id, tab_id, title, now_ms)?;
    Ok(created)
}

fn lane_update_field(body: &Value) -> Result<Option<TaskLaneUpdate>, String> {
    let Some(lane) = body.get("lane") else {
        return Ok(None);
    };
    let mut update = TaskLaneUpdate::default();
    update.tab_id = lane.get("tab_id").map(|v| v.as_str().map(str::to_string));
    update.workspace_id = lane
        .get("workspace_id")
        .map(|v| v.as_str().map(str::to_string));
    update.workspace_name = lane
        .get("workspace_name")
        .map(|v| v.as_str().map(str::to_string));
    update.worktree_path = lane
        .get("worktree_path")
        .map(|v| v.as_str().map(str::to_string));
    update.branch = lane.get("branch").map(|v| v.as_str().map(str::to_string));
    update.base = lane.get("base").map(|v| v.as_str().map(str::to_string));
    update.destination = lane
        .get("destination")
        .map(|v| v.as_str().map(str::to_string));
    update.cleanup_status = lane
        .get("cleanup_status")
        .map(|v| v.as_str().map(str::to_string));
    if let Some(hints) = lane.get("cleanup_hints") {
        update.cleanup_hints = Some(
            serde_json::from_value(hints.clone())
                .map_err(|e| format!("invalid cleanup_hints: {e}"))?,
        );
    }
    Ok((!update.is_empty()).then_some(update))
}

fn push_hints(value: &mut Value, hints: Vec<String>) {
    let hints: Vec<String> = hints
        .into_iter()
        .filter(|hint| !hint.trim().is_empty())
        .collect();
    if hints.is_empty() {
        return;
    }
    value["_hints"] = serde_json::json!(hints);
}

fn optional_string_vec_field(
    body: &Value,
    key: &str,
    alias: &str,
) -> Result<Option<Vec<String>>, String> {
    let field = body.get(key).or_else(|| body.get(alias));
    let Some(field) = field else {
        return Ok(None);
    };
    if field.is_null() {
        return Ok(Some(Vec::new()));
    }
    serde_json::from_value(field.clone())
        .map(Some)
        .map_err(|e| format!("invalid {key}: {e}"))
}

#[cfg(test)]
mod dependency_field_tests {
    use super::optional_string_vec_field;
    use serde_json::json;

    #[test]
    fn omitted_dependency_field_is_none() {
        assert_eq!(
            optional_string_vec_field(&json!({}), "depends_on", "dependency_task_ids").unwrap(),
            None
        );
    }

    #[test]
    fn dependency_field_empty_or_null_clears() {
        assert_eq!(
            optional_string_vec_field(
                &json!({ "depends_on": [] }),
                "depends_on",
                "dependency_task_ids"
            )
            .unwrap(),
            Some(Vec::new())
        );
        assert_eq!(
            optional_string_vec_field(
                &json!({ "depends_on": null }),
                "depends_on",
                "dependency_task_ids"
            )
            .unwrap(),
            Some(Vec::new())
        );
    }
}

#[derive(Deserialize, Default)]
pub(crate) struct TaskAddBody {
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    sections: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    why: Option<String>,
    #[serde(default)]
    acceptance_criteria: Vec<String>,
    #[serde(default)]
    test_plan: Option<String>,
    #[serde(default)]
    qa_plan: Option<String>,
    #[serde(default)]
    correctness: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    role_guidance: Option<String>,
    #[serde(default)]
    executor_role: Option<String>,
    #[serde(default)]
    personality: Option<String>,
    #[serde(default)]
    personality_provenance: Option<IdentityProvenance>,
    #[serde(default)]
    competition: Option<TaskCompetitionMetadata>,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default, alias = "dependency_task_ids")]
    depends_on: Vec<String>,
    #[serde(default)]
    assign_tab: Option<String>,
    #[serde(default)]
    lane: TaskLaneMetadata,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    priority: Option<crate::plan::TaskPriority>,
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    force: bool,
}

pub async fn task_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TaskAddBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let requested_sections = body
        .sections
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(|x| x.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    let section_ids = state
        .svc
        .plan_store
        .resolve_task_section_ids(&id, &requested_sections)
        .map_err(plan_err)?;
    let kind = if body.kind.is_empty() {
        "implement".to_string()
    } else {
        body.kind.clone()
    };
    state
        .svc
        .plan_store
        .ensure_task_creation_allowed(&id, &kind, body.force)
        .map_err(plan_err)?;
    let has_sections = !section_ids.is_empty();
    let has_model = body.model.as_deref().is_some_and(|s| !s.trim().is_empty());
    let has_why = body.why.as_deref().is_some_and(|s| !s.trim().is_empty());
    let has_criteria = body
        .acceptance_criteria
        .iter()
        .any(|s| !s.trim().is_empty());
    let has_test_plan = body
        .test_plan
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let has_correctness = body
        .correctness
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let tid = state
        .svc
        .plan_store
        .add_task(
            &id,
            NewTask {
                name: body.name,
                kind,
                section_ids,
                model: body.model,
                model_rationale: body.why,
                acceptance_criteria: body.acceptance_criteria,
                test_plan: body.test_plan,
                qa_plan: body.qa_plan,
                correctness: body.correctness,
                body: body.body.unwrap_or_default(),
                role_guidance: body.role_guidance,
                executor_role: body.executor_role,
                personality: body.personality,
                personality_provenance: body.personality_provenance,
                competition: body.competition,
                intent: body.intent,
                parent_task_id: body.parent,
                depends_on: body.depends_on,
                assigned_tab_id: body.assign_tab,
                lane: body.lane,
                group: body.group,
                priority: body.priority,
                handle: body.handle,
                tags: body.tags,
            },
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({
        "task_id": tid.clone(),
        "_hints": crate::service::plan::hints::task_create_hints(
            &id,
            &tid,
            has_sections,
            has_model,
            has_why,
            has_criteria,
            has_test_plan,
            has_correctness
        )
    })))
}

pub async fn task_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let section_ids = if body.get("sections").is_some() {
        let requested_sections = body
            .get("sections")
            .and_then(|v| v.as_str())
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Some(
            state
                .svc
                .plan_store
                .resolve_task_section_ids(&id, &requested_sections)
                .map_err(plan_err)?,
        )
    } else {
        None
    };
    let tags: Vec<String> = body
        .get("tags")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let lane = lane_update_field(&body)
        .map_err(|message| text_err(axum::http::StatusCode::BAD_REQUEST, message))?;
    let depends_on = optional_string_vec_field(&body, "depends_on", "dependency_task_ids")
        .map_err(|message| text_err(axum::http::StatusCode::BAD_REQUEST, message))?;
    let acceptance_criteria = body.get("acceptance_criteria").map(|_| {
        body.get("acceptance_criteria")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    });
    let test_plan = optional_string_field(&body, "test_plan");
    let qa_plan = optional_string_field(&body, "qa_plan");
    let correctness = optional_string_field(&body, "correctness");
    state
        .svc
        .plan_store
        .update_task_with_lane(
            &id,
            &tid,
            body.get("name").and_then(|v| v.as_str()),
            body.get("type").and_then(|v| v.as_str()),
            optional_string_field(&body, "model"),
            optional_string_field(&body, "why"),
            body.get("status").and_then(|v| v.as_str()),
            optional_string_field(&body, "status_label"),
            body.get("priority").and_then(|v| v.as_str()),
            body.get("body").and_then(|v| v.as_str()),
            optional_string_field(&body, "intent"),
            optional_string_field(&body, "executor_role"),
            section_ids,
            body.get("parent_task_id").map(|v| v.as_str()),
            optional_string_field(&body, "group"),
            &tags,
            body.get("primary").and_then(|v| v.as_str()),
            body.get("handle").and_then(|v| v.as_str()),
            lane,
            epoch_ms(),
        )
        .map_err(plan_err)?;
    if let Some(depends_on) = depends_on {
        state
            .svc
            .plan_store
            .update_task_dependencies(&id, &tid, depends_on, epoch_ms())
            .map_err(plan_err)?;
    }
    if acceptance_criteria.is_some()
        || test_plan.is_some()
        || qa_plan.is_some()
        || correctness.is_some()
    {
        state
            .svc
            .plan_store
            .update_task_quality(
                &id,
                &tid,
                acceptance_criteria,
                test_plan,
                qa_plan,
                correctness,
                epoch_ms(),
            )
            .map_err(plan_err)?;
    }
    if body.get("role_guidance").is_some() {
        state
            .svc
            .plan_store
            .update_task_role_guidance(
                &id,
                &tid,
                optional_string_field(&body, "role_guidance"),
                epoch_ms(),
            )
            .map_err(plan_err)?;
    }
    if body.get("personality").is_some() || body.get("personality_provenance").is_some() {
        let provenance = body
            .get("personality_provenance")
            .cloned()
            .map(|value| {
                serde_json::from_value(value)
                    .map_err(|e| format!("invalid personality_provenance: {e}"))
            })
            .transpose()
            .map_err(|message| text_err(axum::http::StatusCode::BAD_REQUEST, message))?;
        state
            .svc
            .plan_store
            .update_task_personality(
                &id,
                &tid,
                optional_string_field(&body, "personality"),
                provenance,
                epoch_ms(),
            )
            .map_err(plan_err)?;
    }
    if let Some(value) = body.get("competition") {
        let update = serde_json::from_value(value.clone())
            .map_err(|e| text_err(axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
        state
            .svc
            .plan_store
            .update_task_competition(&id, &tid, update, epoch_ms())
            .map_err(plan_err)?;
    }
    // (orchestrator) directly, so a separate `agent report` isn't needed.
    if body.get("status").and_then(|v| v.as_str()) == Some("in_review") {
        let by_tab = headers
            .get("X-CodeTerm-Tab-Id")
            .and_then(|v| v.to_str().ok());
        let intent = body
            .get("intent")
            .and_then(|v| v.as_str())
            .or_else(|| body.get("status_label").and_then(|v| v.as_str()));
        state
            .svc
            .notify_owner_task_in_review(&id, &tid, by_tab, intent);
    }
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize, Default)]
pub(crate) struct TaskCloseBody {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    from: Option<OrchestratorRef>,
}

pub async fn task_close(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
    Json(body): Json<TaskCloseBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let from = body.from.unwrap_or_else(|| AgentRef::from_tab("unknown"));
    state
        .svc
        .plan_store
        .close_task(&id, &tid, body.result.as_deref(), from, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    let mut resp = serde_json::json!({ "ok": true });
    if let Some(plan) = state.svc.plan_store.get(&id) {
        let cleanup = crate::service::plan::cleanup_debt::detect_plan_cleanup_debt(&plan, |tab| {
            state.svc.tab_probe(tab).exists
        });
        let hints = crate::service::plan::cleanup_debt::cleanup_debt_hints(
            &cleanup
                .into_iter()
                .filter(|item| item.task_id == tid)
                .collect::<Vec<_>>(),
        );
        push_hints(&mut resp, hints);
    }
    Ok(Json(resp))
}

#[derive(Deserialize)]
pub(crate) struct TaskAssignBody {
    tab: String,
    #[serde(default)]
    peer: Option<String>,
}

pub async fn task_assign(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
    Json(body): Json<TaskAssignBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .assign_task_with_role(
            &id,
            &tid,
            &body.tab,
            body.peer.as_deref(),
            planner_role_for_tab(&state, &body.tab),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn task_attempts(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let attempts = state
        .svc
        .plan_store
        .list_task_attempts(&id, &tid)
        .map_err(plan_err)?;
    Ok(Json(serde_json::json!({
        "plan_id": id,
        "task_id": tid,
        "attempts": attempts
    })))
}

pub async fn task_attempt_show(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid, attempt_id)): Path<(String, String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let attempt = state
        .svc
        .plan_store
        .show_task_attempt(&id, &tid, &attempt_id)
        .map_err(plan_err)?;
    Ok(Json(serde_json::json!({
        "plan_id": id,
        "task_id": tid,
        "attempt": attempt
    })))
}

pub async fn task_respawn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
    Json(body): Json<TaskRespawnRequest>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let draft = state
        .svc
        .plan_store
        .build_task_respawn_draft(&id, &tid, body)
        .map_err(plan_err)?;
    let spawn = state
        .svc
        .spawn_agent(
            crate::service::SpawnAgentParams {
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
                ..Default::default()
            },
            None,
        )
        .await
        .map_err(|e| text_err(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let created = record_respawn_attempt_and_persist_identity(
        &state,
        &id,
        &tid,
        &spawn.tab_id,
        &draft,
        &spawn.title,
        epoch_ms(),
    )
    .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({
        "ok": true,
        "plan_id": id,
        "task_id": tid,
        "tab_id": spawn.tab_id,
        "spawn": spawn,
        "attempt": created.attempt,
        "draft": draft
    })))
}

pub async fn task_rm(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .rm_task(&id, &tid, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_hold::TimedRwLock;
    use crate::multiplexer::state::MultiplexerState;
    use crate::plan::{OrchestratorRef, Plan, TaskLaneMetadata};
    use crate::service::plan::NewTask;
    use std::sync::Arc;
    #[test]
    fn rest_respawn_persists_spawned_attempt_title_and_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::service::plan::PlanStore::new_for_test(dir.path());
        let (mux_state, _) = MultiplexerState::new();
        let mux = Arc::new(TimedRwLock::new("mux", mux_state));
        let mut plan = Plan::new(
            "Respawn".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "task".into(),
                    kind: "code".into(),
                    lane: TaskLaneMetadata {
                        workspace_id: Some("ws-rest".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let source_draft = crate::service::plan::task_attempts::TaskRespawnDraft {
            plan_id: plan.id.clone(),
            task_id: task_id.clone(),
            source_attempt: Default::default(),
            provider: "test-provider".into(),
            planned_model: None,
            model: None,
            parent_tab_id: None,
            workspace_id: Some("ws-rest".into()),
            title: None,
            explicit_title: false,
            cwd: dir.path().display().to_string(),
            task_prompt: "prompt".into(),
            report_command: "report".into(),
        };
        let created = store
            .record_task_respawn_attempt(
                &plan.id,
                &task_id,
                "source-tab",
                &source_draft,
                Some("worker"),
                3,
            )
            .unwrap();
        let ws = mux
            .write()
            .create_workspace("REST".into(), Some(dir.path().display().to_string()))
            .id;
        let tab_id = mux.write().create_tab(&ws).unwrap();
        mux.write().get_tab_mut(&tab_id).unwrap().agent_session_id = Some("rest-session".into());
        let draft = store
            .build_task_respawn_draft(
                &plan.id,
                &task_id,
                crate::service::plan::task_attempts::TaskRespawnRequest {
                    attempt_id: created.attempt.id.clone(),
                    provider: None,
                    model: None,
                    parent_tab_id: None,
                    workspace_id: Some("ws-rest".into()),
                    title: Some("Exact User Title".into()),
                },
            )
            .unwrap();
        let persisted = record_respawn_attempt_and_persist_identity_for(
            &store,
            &mux,
            Some("worker"),
            &plan.id,
            &task_id,
            &tab_id,
            &draft,
            "Exact User Title",
            4,
        )
        .unwrap();
        let task = store.show_task(&task_id).unwrap();
        let attempt = task
            .task
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == persisted.attempt.id)
            .unwrap();
        assert_eq!(attempt.executor_title.as_deref(), Some("Exact User Title"));
        assert_eq!(attempt.session_id.as_deref(), Some("rest-session"));
    }
}
