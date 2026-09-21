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
use crate::service::{
    persist_attempt_executor_identity_for_mux, respawn_title_provenance, ServiceState,
};

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
    external_refs: Vec<String>,
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
    execution_intent: crate::plan::TaskExecutionIntent,
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
}

pub async fn task_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TaskAddBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let author = state
        .svc
        .authorize_draft_structure_mutation(
            &id,
            &crate::api::tab_routes::authenticated_plan_caller(&headers),
        )
        .map_err(|message| text_err(axum::http::StatusCode::FORBIDDEN, message))?;
    let execution_fields = serde_json::json!({
        "assign_tab": body.assign_tab.clone(),
        "executor_role": body.executor_role.clone(),
        "role_guidance": body.role_guidance.clone(),
        "personality": body.personality.clone(),
        "personality_provenance": body.personality_provenance.clone(),
        "competition": body.competition.clone(),
        "lane": if body.lane == TaskLaneMetadata::default() {
            serde_json::Value::Null
        } else {
            serde_json::to_value(&body.lane).unwrap_or_default()
        },
    });
    crate::service::ensure_planner_task_fields_are_structural(&author, &execution_fields)
        .map_err(|message| text_err(axum::http::StatusCode::FORBIDDEN, message))?;
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
    let external_refs = body.external_refs.clone();
    let tid = state
        .svc
        .plan_store
        .add_task_with_external_refs(
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
                execution_intent: body.execution_intent,
                parent_task_id: body.parent,
                depends_on: body.depends_on,
                assigned_tab_id: body.assign_tab,
                lane: body.lane,
                group: body.group,
                priority: body.priority,
                handle: body.handle,
                tags: body.tags,
            },
            &external_refs,
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({
        "task_id": tid.clone(),
        "key": state.svc.plan_store.task_display_key(&id, &tid),
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
    verify_task_mutation_owner(&state.svc, &headers, &id, &tid, TaskMutationKind::Recovery)?;
    task_update_core(&state.svc, &headers, &id, &tid, body)
}

fn task_update_core(
    service: &crate::service::ServiceState,
    headers: &HeaderMap,
    id: &str,
    tid: &str,
    body: Value,
) -> PlanRouteResult {
    crate::plan::task_update_fields::validate_task_update_fields(
        &body,
        crate::plan::task_update_fields::TaskUpdateSeam::Rest,
    )
    .map_err(|error| text_err(axum::http::StatusCode::BAD_REQUEST, error))?;
    let author = service
        .authorize_draft_structure_mutation(
            id,
            &crate::api::tab_routes::authenticated_plan_caller(headers),
        )
        .map_err(|message| text_err(axum::http::StatusCode::FORBIDDEN, message))?;
    crate::service::ensure_planner_task_fields_are_structural(&author, &body)
        .map_err(|message| text_err(axum::http::StatusCode::FORBIDDEN, message))?;
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
            service
                .plan_store
                .resolve_task_section_ids(id, &requested_sections)
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
    let lane = crate::service::lane_removal::clear_vanished_lane_on_cleanup_done(
        &service.plan_store,
        id,
        tid,
        lane,
    )
    .map_err(|message| text_err(axum::http::StatusCode::INTERNAL_SERVER_ERROR, message))?;
    let depends_on = optional_string_vec_field(&body, "depends_on", "dependency_task_ids")
        .map_err(|message| text_err(axum::http::StatusCode::BAD_REQUEST, message))?;
    // The edit panel submits the whole array, so this surface is Replace-only.
    let acceptance_criteria = body.get("acceptance_criteria").map(|_| {
        let items: Vec<String> = body
            .get("acceptance_criteria")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        crate::service::plan::mutations::task_ops::AcceptanceCriteriaUpdate::Replace(items)
    });
    let external_refs = body
        .get("external_refs")
        .map(|value| serde_json::from_value::<Vec<String>>(value.clone()))
        .transpose()
        .map_err(|error| {
            text_err(
                axum::http::StatusCode::BAD_REQUEST,
                format!("invalid external_refs: expected an array of reference strings: {error}"),
            )
        })?
        .unwrap_or_default();
    crate::service::plan::external_refs::canonicalize_external_refs(&external_refs)
        .map_err(plan_err)?;
    let test_plan = optional_string_field(&body, "test_plan");
    let qa_plan = optional_string_field(&body, "qa_plan");
    let correctness = optional_string_field(&body, "correctness");
    service
        .plan_store
        .update_task_with_lane(
            id,
            tid,
            body.get("name").and_then(|v| v.as_str()),
            body.get("type")
                .or_else(|| body.get("kind"))
                .and_then(|v| v.as_str()),
            optional_string_field(&body, "model"),
            optional_string_field(&body, "why"),
            body.get("status").and_then(|v| v.as_str()),
            optional_string_field(&body, "status_label"),
            body.get("priority").and_then(|v| v.as_str()),
            body.get("body").and_then(|v| v.as_str()),
            optional_string_field(&body, "intent"),
            optional_string_field(&body, "executor_role"),
            section_ids,
            body.get("parent_task_id")
                .or_else(|| body.get("parent"))
                .map(|v| v.as_str()),
            optional_string_field(&body, "group"),
            &tags,
            body.get("primary").and_then(|v| v.as_str()),
            body.get("handle").and_then(|v| v.as_str()),
            lane,
            epoch_ms(),
        )
        .map_err(plan_err)?;
    if let Some(depends_on) = depends_on {
        service
            .plan_store
            .update_task_dependencies(id, tid, depends_on, epoch_ms())
            .map_err(plan_err)?;
    }
    if acceptance_criteria.is_some()
        || test_plan.is_some()
        || qa_plan.is_some()
        || correctness.is_some()
    {
        service
            .plan_store
            .update_task_quality(
                id,
                tid,
                acceptance_criteria,
                test_plan,
                qa_plan,
                correctness,
                epoch_ms(),
            )
            .map_err(plan_err)?;
    }
    if body.get("role_guidance").is_some() {
        service
            .plan_store
            .update_task_role_guidance(
                id,
                tid,
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
        service
            .plan_store
            .update_task_personality(
                id,
                tid,
                optional_string_field(&body, "personality"),
                provenance,
                epoch_ms(),
            )
            .map_err(plan_err)?;
    }
    if let Some(value) = body.get("competition") {
        let update = serde_json::from_value(value.clone())
            .map_err(|e| text_err(axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
        service
            .plan_store
            .update_task_competition(id, tid, update, epoch_ms())
            .map_err(plan_err)?;
    }
    if !external_refs.is_empty() {
        service
            .plan_store
            .add_task_external_refs(id, tid, &external_refs, epoch_ms())
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
        service.notify_owner_task_in_review(id, tid, by_tab, intent);
    }
    crate::api::events::emit_everywhere(
        &service.app_handle,
        "plan-changed",
        serde_json::json!({ "plan_id": id }),
    );
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
    verify_task_mutation_owner(&state.svc, &headers, &id, &tid, TaskMutationKind::Recovery)?;
    let from = body.from.unwrap_or_else(|| AgentRef::from_tab("unknown"));
    let owner_epoch = state
        .svc
        .plan_store
        .show_task(&tid)
        .map(|detail| detail.task.interruption.owner_epoch)
        .unwrap_or(0);
    if owner_epoch > 0 {
        let authority = rest_task_mutation_authority(&headers)?;
        let mux = state.svc.mux.read();
        if !mux.get_tab(&authority.tab_id).is_some_and(|tab| {
            tab.agent_session_id.as_deref() == Some(&authority.session_id)
                && tab.session_binding_nonce.as_deref() == Some(&authority.session_binding_nonce)
                && tab.reported_session_binding_nonce.as_deref()
                    == Some(&authority.session_binding_nonce)
        }) {
            return Err(text_err(
                axum::http::StatusCode::CONFLICT,
                "task mutation caller rebound",
            ));
        }
        state
            .svc
            .plan_store
            .close_task_authorized(
                &id,
                &tid,
                body.result.as_deref(),
                from,
                &authority,
                epoch_ms(),
            )
            .map_err(plan_err)?;
        drop(mux);
    } else {
        state
            .svc
            .plan_store
            .close_task(&id, &tid, body.result.as_deref(), from, epoch_ms())
            .map_err(plan_err)?;
    }
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
        .authorize_execution_action(
            &id,
            &crate::api::tab_routes::authenticated_plan_caller(&headers),
        )
        .map_err(|message| text_err(axum::http::StatusCode::FORBIDDEN, message))?;
    verify_task_mutation_owner(&state.svc, &headers, &id, &tid, TaskMutationKind::Recovery)?;
    let owner_epoch = state
        .svc
        .plan_store
        .show_task(&tid)
        .map(|detail| detail.task.interruption.owner_epoch)
        .unwrap_or(0);
    let authority = if owner_epoch > 0 {
        Some(rest_task_mutation_authority(&headers)?)
    } else {
        None
    };
    {
        let mux = state.svc.mux.read();
        crate::service::task_assignment::assign_existing_task(
            &state.svc.plan_store,
            &mux,
            &id,
            &tid,
            &body.tab,
            body.peer.as_deref(),
            authority.as_ref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    }
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

fn rest_task_mutation_authority(
    headers: &HeaderMap,
) -> Result<
    crate::service::plan::interruption::TaskMutationAuthority,
    (axum::http::StatusCode, Json<Value>),
> {
    let required = |name: &'static str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                text_err(
                    axum::http::StatusCode::UNAUTHORIZED,
                    format!("missing {name}"),
                )
            })
    };
    Ok(crate::service::plan::interruption::TaskMutationAuthority {
        tab_id: required("X-CodeTerm-Tab-Id")?,
        session_id: required("X-CodeTerm-Session-Id")?,
        session_binding_nonce: required("X-CodeTerm-Session-Binding-Nonce")?,
        owner_epoch: headers
            .get("X-CodeTerm-Owner-Epoch")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| {
                text_err(axum::http::StatusCode::CONFLICT, "missing task owner epoch")
            })?,
    })
}

/// REST twin of the socket gate's recovery classification: only these verbs may
/// be widened for the live Plan owner. `task_rm` is deliberately excluded.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskMutationKind {
    Recovery,
    Ordinary,
}

fn owner_tab_is_gone(svc: &ServiceState, owner_tab_id: Option<&str>) -> bool {
    owner_tab_id.is_none_or(|tab_id| svc.mux.read().get_tab(tab_id).is_none())
}

pub(super) fn verify_task_mutation_owner(
    svc: &ServiceState,
    headers: &HeaderMap,
    plan_id: &str,
    task_id: &str,
    recovery: TaskMutationKind,
) -> Result<(), (axum::http::StatusCode, Json<Value>)> {
    let detail = svc
        .plan_store
        .show_task(task_id)
        .filter(|detail| detail.plan_id == plan_id)
        .ok_or_else(|| plan_err(crate::service::plan::PlanError::TaskNotFound))?;
    if detail.task.interruption.owner_epoch == 0 {
        return Ok(());
    }
    if recovery == TaskMutationKind::Recovery
        && owner_tab_is_gone(svc, detail.task.interruption.owner_tab_id.as_deref())
        && svc
            .authorize_plan_orchestrator(
                plan_id,
                &crate::api::tab_routes::authenticated_plan_caller(headers),
            )
            .is_ok()
    {
        return svc
            .plan_store
            .retire_stale_task_owner(plan_id, task_id, None, |_| false, epoch_ms())
            .map(|_| ())
            .map_err(|error| text_err(axum::http::StatusCode::CONFLICT, error.to_string()));
    }
    let tab_id = headers
        .get("X-CodeTerm-Tab-Id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            text_err(
                axum::http::StatusCode::UNAUTHORIZED,
                "missing Worker tab id",
            )
        })?;
    let session_id = headers
        .get("X-CodeTerm-Session-Id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            text_err(
                axum::http::StatusCode::UNAUTHORIZED,
                "missing Worker session id",
            )
        })?;
    let nonce = headers
        .get("X-CodeTerm-Session-Binding-Nonce")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            text_err(
                axum::http::StatusCode::UNAUTHORIZED,
                "missing session binding nonce",
            )
        })?;
    let caller_epoch = headers
        .get("X-CodeTerm-Owner-Epoch")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| text_err(axum::http::StatusCode::CONFLICT, "missing task owner epoch"))?;
    if detail.task.interruption.owner_epoch != caller_epoch {
        return Err(text_err(
            axum::http::StatusCode::CONFLICT,
            "task mutation owner epoch is stale",
        ));
    }
    let binding_matches = svc.mux.read().get_tab(tab_id).is_some_and(|tab| {
        tab.agent_session_id.as_deref() == Some(session_id)
            && tab.session_binding_nonce.as_deref() == Some(nonce)
            && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
    });
    if !binding_matches {
        return Err(text_err(
            axum::http::StatusCode::CONFLICT,
            "task mutation caller binding is stale or rebound",
        ));
    }
    if svc
        .authorize_plan_orchestrator(
            plan_id,
            &crate::api::tab_routes::authenticated_plan_caller(headers),
        )
        .is_ok()
    {
        return Ok(());
    }
    svc.verify_task_owner_session(task_id, tab_id, session_id, caller_epoch)
        .map_err(|error| text_err(axum::http::StatusCode::CONFLICT, error))
}

pub async fn task_adopt_checkpoint(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let tab_id = headers
        .get("X-CodeTerm-Tab-Id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| text_err(axum::http::StatusCode::BAD_REQUEST, "missing Worker tab id"))?;
    let session_id = state
        .svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_session_id.clone())
        .ok_or_else(|| {
            text_err(
                axum::http::StatusCode::CONFLICT,
                "adoption caller has no authenticated session",
            )
        })?;
    let detail = state
        .svc
        .plan_store
        .show_task(&tid)
        .filter(|detail| detail.plan_id == id)
        .ok_or_else(|| plan_err(crate::service::plan::PlanError::TaskNotFound))?;
    let (owner_epoch, attempt_id) = state
        .svc
        .adopt_worker_checkpoint(&detail.task.id, tab_id, &session_id, epoch_ms())
        .await
        .map_err(|error| text_err(axum::http::StatusCode::CONFLICT, error))?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({
        "ok": true,
        "plan_id": id,
        "task_id": tid,
        "attempt_id": attempt_id,
        "owner_epoch": owner_epoch,
        "tab_id": tab_id,
        "session_id": session_id,
        "state": "awaiting_worker_ack"
    })))
}

const RESPAWN_ATTEMPT_FLAG: &str = "--attempt";

// `plan task respawn` requires the source attempt, so a hint without it names a
// command the CLI rejects.
fn respawn_spawn_command(plan_id: &str, task_id: &str, source_attempt_id: &str) -> String {
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
    state
        .svc
        .authorize_execution_action(
            &id,
            &crate::api::tab_routes::authenticated_plan_caller(&headers),
        )
        .map_err(|message| text_err(axum::http::StatusCode::FORBIDDEN, message))?;
    verify_task_mutation_owner(&state.svc, &headers, &id, &tid, TaskMutationKind::Recovery)?;
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
    let parent_tab_id = created
        .attempt
        .orchestrator_tab_id
        .as_deref()
        .or(created.attempt.parent_tab_id.as_deref());
    if let Some(parent_tab_id) = parent_tab_id {
        state.svc.arm_worker_ack_deadline(
            parent_tab_id,
            &spawn.tab_id,
            &spawn.title,
            &id,
            &tid,
            &created.attempt.id,
            respawn_spawn_command(&id, &tid, &draft.source_attempt.id),
        );
    }
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
    verify_task_mutation_owner(&state.svc, &headers, &id, &tid, TaskMutationKind::Ordinary)?;
    state
        .svc
        .authorize_draft_structure_mutation(
            &id,
            &crate::api::tab_routes::authenticated_plan_caller(&headers),
        )
        .map_err(|message| text_err(axum::http::StatusCode::FORBIDDEN, message))?;
    state
        .svc
        .plan_store
        .rm_task(&id, &tid, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[cfg(test)]
pub(super) mod tests;
