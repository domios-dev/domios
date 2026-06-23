use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::api::routes::{check_auth, AppState};
use crate::plan::{AgentRef, MilestoneKind, OrchestratorRef, PlanStatus, TaskGroup};
use crate::service::plan::{NewTask, PlanError};

#[cfg(test)]
mod tests;

type PlanRouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn text_err(status: StatusCode, message: impl ToString) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(serde_json::json!({ "error": message.to_string() })),
    )
}

fn plan_err(e: PlanError) -> (StatusCode, Json<Value>) {
    let status = match e {
        PlanError::NotFound | PlanError::TaskNotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::BAD_REQUEST,
    };
    let body = serde_json::json!({
        "error": e.to_string(),
        "plan_error": e.to_json(),
    });
    (status, Json(body))
}

fn emit_plan_changed(state: &AppState, plan_id: &str) {
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "plan-changed",
        serde_json::json!({ "plan_id": plan_id }),
    );
}

fn kinds_from_csv(raw: &str) -> Vec<MilestoneKind> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|key| MilestoneKind {
            key: key.into(),
            label: key.into(),
            color: None,
        })
        .collect()
}

// Parse `key:Label,key2:Label2` into task groups; missing label defaults to key.
fn groups_from_csv(raw: &str) -> Vec<TaskGroup> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (key, label) = item.split_once(':').unwrap_or((item, item));
            TaskGroup {
                key: key.trim().into(),
                label: label.trim().into(),
                color: None,
            }
        })
        .collect()
}

#[derive(Deserialize, Default)]
pub(crate) struct CreatePlanBody {
    title: String,
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    owner: Option<OrchestratorRef>,
    #[serde(default)]
    kinds: Option<String>,
    #[serde(default)]
    spec: Option<String>,
}

// Query bools must accept "1" as well as "true" — serde's bool only takes
// "true"/"false", so a raw `Option<bool>` 400s on `?full=1` (the form the UI sends).
fn de_truthy<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    let s = <Option<String> as serde::Deserialize>::deserialize(d)?;
    Ok(s.map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on")))
}

#[derive(Deserialize, Default)]
pub(crate) struct ListPlansQuery {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    orchestrator: Option<String>,
    #[serde(default, deserialize_with = "de_truthy")]
    unowned: Option<bool>,
    #[serde(default, deserialize_with = "de_truthy")]
    full: Option<bool>,
}

pub async fn create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreatePlanBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let project = body.project_path.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    });
    let owner = body
        .owner
        .unwrap_or_else(|| OrchestratorRef::from_pane("unknown"));
    let kinds = body
        .kinds
        .as_deref()
        .map(kinds_from_csv)
        .unwrap_or_default();
    let now = epoch_ms();
    let plan = state
        .svc
        .plan_store
        .create_plan(body.title, project, owner, kinds, now)
        .map_err(plan_err)?;
    if let Some(spec) = body.spec.filter(|s| !s.is_empty()) {
        let _ = state
            .svc
            .plan_store
            .add_section(&plan.id, "Spec", &spec, None, now);
    }
    emit_plan_changed(&state, &plan.id);
    Ok(Json(
        serde_json::json!({ "plan_id": plan.id, "plan": plan }),
    ))
}

pub async fn list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListPlansQuery>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let want = if let Some(project) = &q.project {
        Some(
            state
                .svc
                .ensure_plan_project_loaded(std::path::Path::new(project))
                .map_err(plan_err)?
                .display()
                .to_string(),
        )
    } else {
        None
    };
    let mut entries = state.svc.plan_store.list_index();
    if let Some(want) = &want {
        entries.retain(|e| {
            state
                .svc
                .plan_store
                .get(&e.plan_id)
                .map(|p| p.project_path == *want)
                .unwrap_or(false)
        });
    }
    if let Some(status) = &q.status {
        entries.retain(|e| match status.as_str() {
            "active" => e.status == PlanStatus::Active,
            "archived" => e.status == PlanStatus::Archived,
            _ => true,
        });
    }
    if q.unowned == Some(true) {
        entries.retain(|e| e.owner.pane_id.is_none() && e.owner.session_id.is_none());
    }
    if let Some(orch) = &q.orchestrator {
        entries.retain(|e| {
            e.owner.pane_id.as_deref() == Some(orch.as_str())
                || e.owner.session_id.as_deref() == Some(orch.as_str())
        });
    }
    if q.full == Some(true) {
        let plans: Vec<_> = entries
            .iter()
            .filter_map(|e| state.svc.plan_store.get(&e.plan_id))
            .collect();
        return Ok(Json(serde_json::json!({ "plans": plans })));
    }
    Ok(Json(serde_json::json!({ "plans": entries })))
}

pub async fn get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let plan = state
        .svc
        .plan_store
        .get(&id)
        .ok_or_else(|| plan_err(PlanError::NotFound))?;
    Ok(Json(serde_json::to_value(plan).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
pub(crate) struct UpdatePlanBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

pub async fn update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdatePlanBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let status = body
        .status
        .as_deref()
        .map(|s| serde_json::from_value::<PlanStatus>(serde_json::Value::String(s.into())))
        .transpose()
        .map_err(|_| plan_err(PlanError::BadStatus))?;
    let now = epoch_ms();
    state
        .svc
        .plan_store
        .update_plan(&id, body.title.as_deref(), status, now)
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_plan(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .delete(&id)
        .map_err(|e| text_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn archive(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .archive(&id, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct TransferBody {
    to: OrchestratorRef,
    #[serde(default)]
    reason: String,
}

pub async fn transfer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TransferBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .transfer(&id, body.to, &body.reason, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn disown(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .disown(&id, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct ClaimBody {
    owner: OrchestratorRef,
}

pub async fn claim(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ClaimBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .claim(&id, body.owner, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct SectionAddBody {
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    parent: Option<String>,
}

pub async fn section_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SectionAddBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let sid = state
        .svc
        .plan_store
        .add_section(
            &id,
            &body.title,
            &body.body,
            body.parent.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "section_id": sid })))
}

#[derive(Deserialize, Default)]
pub(crate) struct SectionUpdateBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
}

pub async fn section_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, sid)): Path<(String, String)>,
    Json(body): Json<SectionUpdateBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .update_section(
            &id,
            &sid,
            body.title.as_deref(),
            body.body.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn section_rm(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, sid)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .rm_section(&id, &sid, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct KindsSetBody {
    kinds: String,
}

pub async fn kinds_set(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<KindsSetBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .set_kinds(&id, kinds_from_csv(&body.kinds), epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
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
    body: Option<String>,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    assign_pane: Option<String>,
    #[serde(default)]
    group: Option<String>,
}

pub async fn task_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TaskAddBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let section_ids = body
        .sections
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(|x| x.to_string())
                .collect()
        })
        .unwrap_or_default();
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
                body: body.body.unwrap_or_default(),
                intent: body.intent,
                assigned_pane_id: body.assign_pane,
                group: body.group,
            },
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "task_id": tid })))
}

#[derive(Deserialize, Default)]
pub(crate) struct TaskUpdateBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    why: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    status_label: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    sections: Option<String>,
    #[serde(default)]
    group: Option<String>,
}

pub async fn task_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
    Json(body): Json<TaskUpdateBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let section_ids = body.sections.as_ref().map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(|x| x.to_string())
            .collect()
    });
    state
        .svc
        .plan_store
        .update_task(
            &id,
            &tid,
            body.name.as_deref(),
            body.kind.as_deref(),
            Some(body.model.as_deref()),
            Some(body.why.as_deref()),
            body.status.as_deref(),
            Some(body.status_label.as_deref()),
            body.body.as_deref(),
            Some(body.intent.as_deref()),
            section_ids,
            body.group.as_ref().map(|s| Some(s.as_str())),
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

#[derive(Deserialize)]
pub(crate) struct MilestoneAddBody {
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    add_kind: bool,
    #[serde(default)]
    from: Option<OrchestratorRef>,
}

pub async fn milestone_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
    Json(body): Json<MilestoneAddBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let from = body.from.unwrap_or_else(|| AgentRef::from_pane("unknown"));
    let mid = state
        .svc
        .plan_store
        .add_milestone(
            &id,
            &tid,
            &body.kind,
            &body.text,
            from,
            body.add_kind,
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "milestone_id": mid })))
}

#[derive(Deserialize, Default)]
pub(crate) struct MilestoneUpdateBody {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

pub async fn milestone_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid, ms_id)): Path<(String, String, String)>,
    Json(body): Json<MilestoneUpdateBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .update_milestone(
            &id,
            &tid,
            &ms_id,
            body.kind.as_deref(),
            body.text.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn milestone_rm(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid, ms_id)): Path<(String, String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .rm_milestone(&id, &tid, &ms_id, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct LoopCreateBody {
    target: String,
    budget: u32,
    #[serde(default)]
    stages: Vec<crate::plan::LoopStage>,
    #[serde(default)]
    stop_when: Option<String>,
}

pub async fn loop_create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LoopCreateBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let lid = state
        .svc
        .plan_store
        .create_loop(
            &id,
            &body.target,
            body.stages,
            body.budget,
            body.stop_when.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "loop_id": lid })))
}

#[derive(Deserialize, Default)]
pub(crate) struct LoopSetBody {
    #[serde(default)]
    budget: Option<u32>,
    #[serde(default)]
    stop_when: Option<String>,
}

pub async fn loop_set(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((_id, lid)): Path<(String, String)>,
    Json(body): Json<LoopSetBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .set_loop(
            &lid,
            body.budget,
            Some(body.stop_when.as_deref()),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    if let Some(plan_id) = state.svc.plan_store.list_index().iter().find_map(|e| {
        state
            .svc
            .plan_store
            .get(&e.plan_id)
            .filter(|p| p.loops.iter().any(|l| l.id == lid))
            .map(|p| p.id)
    }) {
        emit_plan_changed(&state, &plan_id);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn loop_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((_id, lid)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let spec = state
        .svc
        .plan_store
        .loop_status(&lid)
        .ok_or_else(|| plan_err(PlanError::NotFound))?;
    Ok(Json(serde_json::to_value(spec).unwrap_or_default()))
}

pub async fn loop_stop(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((_id, lid)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .stop_loop(&lid, epoch_ms())
        .map_err(plan_err)?;
    if let Some(plan_id) = state.svc.plan_store.list_index().iter().find_map(|e| {
        state
            .svc
            .plan_store
            .get(&e.plan_id)
            .filter(|p| p.loops.iter().any(|l| l.id == lid))
            .map(|p| p.id)
    }) {
        emit_plan_changed(&state, &plan_id);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct LayoutSetBody {
    element: String,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
}

pub async fn layout_set(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LayoutSetBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .set_layout(&id, &body.element, body.x, body.y, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize, Default)]
pub(crate) struct NoteAddBody {
    #[serde(default)]
    text: String,
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
}

pub async fn note_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<NoteAddBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let eid = state
        .svc
        .plan_store
        .add_note(&id, &body.text, body.links, body.x, body.y, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "element_id": eid })))
}

#[derive(Deserialize)]
pub(crate) struct ElementAddBody {
    kind: String,
    #[serde(default)]
    content: Value,
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
}

pub async fn element_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ElementAddBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let eid = state
        .svc
        .plan_store
        .add_element(
            &id,
            &body.kind,
            body.content,
            body.links,
            body.x,
            body.y,
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "element_id": eid })))
}

pub async fn coverage(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let report = state
        .svc
        .plan_store
        .coverage_report(&id)
        .ok_or_else(|| plan_err(PlanError::NotFound))?;
    Ok(Json(serde_json::to_value(report).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct GroupsSetBody {
    groups: String,
}

pub async fn groups_set(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<GroupsSetBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .set_task_groups(&id, groups_from_csv(&body.groups), epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct LinkBody {
    to: String,
}

pub async fn link(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LinkBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .link_plan(&id, &body.to, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn unlink(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, to)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .unlink_plan(&id, &to, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}
