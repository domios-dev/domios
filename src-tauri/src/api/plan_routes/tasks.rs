use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::api::routes::{check_auth, AppState};
use crate::plan::{AgentRef, OrchestratorRef};
use crate::service::plan::NewTask;

use super::{
    emit_plan_changed, epoch_ms, optional_string_field, plan_err, text_err, PlanRouteResult,
};

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
    correctness: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    assign_pane: Option<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    priority: Option<crate::plan::TaskPriority>,
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
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
                kind: if body.kind.is_empty() {
                    "implement".into()
                } else {
                    body.kind
                },
                section_ids,
                model: body.model,
                model_rationale: body.why,
                acceptance_criteria: body.acceptance_criteria,
                test_plan: body.test_plan,
                correctness: body.correctness,
                body: body.body.unwrap_or_default(),
                intent: body.intent,
                parent_task_id: body.parent,
                assigned_pane_id: body.assign_pane,
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
    state
        .svc
        .plan_store
        .update_task(
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
            section_ids,
            body.get("parent_task_id").map(|v| v.as_str()),
            optional_string_field(&body, "group"),
            &tags,
            body.get("primary").and_then(|v| v.as_str()),
            body.get("handle").and_then(|v| v.as_str()),
            epoch_ms(),
        )
        .map_err(plan_err)?;
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
    let from = body.from.unwrap_or_else(|| AgentRef::from_pane("unknown"));
    state
        .svc
        .plan_store
        .close_task(&id, &tid, body.result.as_deref(), from, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct TaskAssignBody {
    pane: String,
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
        .assign_task(&id, &tid, &body.pane, body.peer.as_deref(), epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
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
