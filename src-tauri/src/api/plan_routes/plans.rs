use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path as FsPath, PathBuf};

use crate::api::routes::{check_auth, AppState};
use crate::plan::{
    OrchestratorRef, PlanMode, PlanStatus, PlannerRef, PlanningAuthor, PlanningState,
};
use crate::service::plan::IndexEntry;
use crate::service::plan::PlanError;
use crate::service::FirstPlanTeamError;
use crate::service::TeamPlanError;

use super::{
    emit_plan_changed, epoch_ms, kinds_from_csv, plan_err, tag_facets_from_body, text_err,
    PlanRouteResult,
};

fn push_hints(value: &mut Value, hints: Vec<String>) {
    let hints: Vec<_> = hints
        .into_iter()
        .filter(|hint| !hint.trim().is_empty())
        .collect();
    if !hints.is_empty() {
        value["_hints"] = serde_json::json!(hints);
    }
}

#[derive(Deserialize, Default)]
pub(crate) struct CreatePlanBody {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) project_path: Option<String>,
    #[serde(default)]
    pub(crate) owner: Option<OrchestratorRef>,
    #[serde(default)]
    pub(crate) kinds: Option<String>,
    #[serde(default)]
    pub(crate) from_spec: Option<String>,
}

fn de_truthy<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    let s = <Option<String> as serde::Deserialize>::deserialize(d)?;
    Ok(s.map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on")))
}

#[derive(Deserialize, Default)]
pub(crate) struct ListPlansQuery {
    #[serde(default)]
    pub(crate) project: Option<String>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) orchestrator: Option<String>,
    #[serde(default, deserialize_with = "de_truthy")]
    pub(crate) unowned: Option<bool>,
    #[serde(default, deserialize_with = "de_truthy")]
    pub(crate) full: Option<bool>,
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
    let from_spec = body.from_spec.filter(|s| !s.trim().is_empty());
    let outcome = state
        .svc
        .create_first_plan_for_orchestrator(
            body.title,
            project,
            owner,
            kinds,
            from_spec.clone(),
            now,
        )
        .map_err(|error| match error {
            FirstPlanTeamError::Plan(error) => plan_err(error),
            FirstPlanTeamError::Team(message) => text_err(StatusCode::BAD_REQUEST, message),
        })?;
    let plan = outcome.plan.clone();
    let _ = state
        .svc
        .bind_pending_paired_planner(&plan.id, &plan.owner, now);
    emit_plan_changed(&state, &plan.id);
    let mut response = serde_json::json!({
        "plan_id": plan.id,
        "plan": plan,
        "from_spec": from_spec.unwrap_or_default()
    });
    outcome.apply_to_response(&mut response);
    Ok(Json(response))
}

pub async fn list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListPlansQuery>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .validate_active_team_plan_bindings()
        .map_err(team_plan_err)?;
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
        if !state.svc.plans_startup_loaded() {
            return Err(text_err(
                StatusCode::SERVICE_UNAVAILABLE,
                "plans are still loading; retry shortly",
            ));
        }
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

#[derive(Deserialize)]
pub(crate) struct ResolveRefsBody {
    refs: Vec<ResolveRefRequest>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResolveRefRequest {
    plan_id: String,
    #[serde(default)]
    target_id: Option<String>,
    target_kind: ResolveTargetKind,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResolveTargetKind {
    Plan,
    Task,
    Section,
}

#[derive(Debug, Serialize)]
struct ResolveRefsResponse {
    refs: Vec<ResolvedRef>,
}

#[derive(Debug, Serialize)]
struct ResolvedRef {
    plan_id: String,
    target_id: Option<String>,
    target_kind: ResolveTargetKind,
    state: &'static str,
    title: String,
    plan_title: String,
    task_title: Option<String>,
    section_title: Option<String>,
    file_path: Option<String>,
    status: Option<PlanStatus>,
    owner_pane_id: Option<String>,
    owner_session_id: Option<String>,
}

pub async fn resolve_refs(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<ResolveRefsBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let index = state.svc.plan_store.list_index();
    let refs = body
        .refs
        .into_iter()
        .map(|item| resolve_ref(&state, &index, item))
        .collect::<Vec<_>>();
    Ok(Json(
        serde_json::to_value(ResolveRefsResponse { refs }).unwrap_or_default(),
    ))
}

fn resolve_ref(state: &AppState, index: &[IndexEntry], item: ResolveRefRequest) -> ResolvedRef {
    let index_entry = index.iter().find(|entry| entry.plan_id == item.plan_id);
    let (plan, missing_state) = load_plan_for_ref(
        &item.plan_id,
        index_entry,
        || state.svc.plan_store.get(&item.plan_id),
        |project_root| {
            state
                .svc
                .ensure_plan_project_loaded(project_root)
                .map(|_| ())
                .map_err(classify_load_error)
        },
    );
    let file_path = index_entry
        .as_ref()
        .map(|entry| entry.file_path.display().to_string());
    let status = index_entry.as_ref().map(|entry| entry.status.clone());
    let owner_pane_id = index_entry
        .as_ref()
        .and_then(|entry| entry.owner.pane_id.clone());
    let owner_session_id = index_entry
        .as_ref()
        .and_then(|entry| entry.owner.session_id.clone());
    let plan_title = plan
        .as_ref()
        .map(|entry| entry.title.trim())
        .filter(|title| !title.is_empty())
        .or_else(|| {
            index_entry
                .as_ref()
                .map(|entry| entry.title.trim())
                .filter(|title| !title.is_empty())
        })
        .unwrap_or(&item.plan_id)
        .to_string();

    let Some(plan) = plan else {
        return ResolvedRef {
            plan_id: item.plan_id,
            target_id: item.target_id,
            target_kind: item.target_kind,
            state: missing_state.unwrap_or("missing"),
            title: plan_title.clone(),
            plan_title,
            task_title: None,
            section_title: None,
            file_path,
            status,
            owner_pane_id,
            owner_session_id,
        };
    };

    let mut state_name = "resolved";
    if plan.status == PlanStatus::Archived {
        state_name = "archived";
    } else if plan.owner.pane_id.is_none() && plan.owner.session_id.is_none() {
        state_name = "unowned";
    }

    match item.target_kind {
        ResolveTargetKind::Plan => ResolvedRef {
            plan_id: item.plan_id,
            target_id: None,
            target_kind: ResolveTargetKind::Plan,
            state: state_name,
            title: plan_title.clone(),
            plan_title,
            task_title: None,
            section_title: None,
            file_path,
            status,
            owner_pane_id,
            owner_session_id,
        },
        ResolveTargetKind::Task => {
            let task = item.target_id.as_deref().and_then(|task_id| {
                plan.tasks
                    .iter()
                    .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
            });
            let task_title = task
                .map(|task| task.name.trim())
                .filter(|title| !title.is_empty())
                .map(str::to_string);
            let title = task_title.clone().unwrap_or_else(|| {
                item.target_id
                    .clone()
                    .unwrap_or_else(|| item.plan_id.clone())
            });
            ResolvedRef {
                plan_id: item.plan_id,
                target_id: item.target_id,
                target_kind: ResolveTargetKind::Task,
                state: if task.is_some() { state_name } else { "stale" },
                title,
                plan_title,
                task_title,
                section_title: None,
                file_path,
                status,
                owner_pane_id,
                owner_session_id,
            }
        }
        ResolveTargetKind::Section => {
            let section = item
                .target_id
                .as_deref()
                .and_then(|section_id| find_section(&plan.sections, section_id));
            let section_title = section
                .map(|section| section.title.trim())
                .filter(|title| !title.is_empty())
                .map(str::to_string);
            let title = section_title.clone().unwrap_or_else(|| {
                item.target_id
                    .clone()
                    .unwrap_or_else(|| item.plan_id.clone())
            });
            ResolvedRef {
                plan_id: item.plan_id,
                target_id: item.target_id,
                target_kind: ResolveTargetKind::Section,
                state: if section.is_some() {
                    state_name
                } else {
                    "stale"
                },
                title,
                plan_title,
                task_title: None,
                section_title,
                file_path,
                status,
                owner_pane_id,
                owner_session_id,
            }
        }
    }
}

fn classify_load_error(error: PlanError) -> &'static str {
    let PlanError::Io(message) = error else {
        return "unreachable";
    };
    let msg = message.to_ascii_lowercase();
    if msg.contains("permission denied") || msg.contains("access is denied") {
        "permission"
    } else {
        "unreachable"
    }
}

fn derive_project_root_from_index_path(file_path: &FsPath, plan_id: &str) -> Option<PathBuf> {
    let file_name = file_path.file_name()?.to_str()?;
    if file_name != "plan.json" && file_name != "plan.yaml" {
        return None;
    }
    let plan_dir = file_path.parent()?;
    if plan_dir.file_name()?.to_str()? != plan_id {
        return None;
    }
    let bucket_dir = plan_dir.parent()?;
    let bucket = bucket_dir.file_name()?.to_str()?;
    if bucket != "active" && bucket != "archive" {
        return None;
    }
    let plans_dir = bucket_dir.parent()?;
    if plans_dir.file_name()?.to_str()? != "plans" {
        return None;
    }
    let codeterm_dir = plans_dir.parent()?;
    if codeterm_dir.file_name()?.to_str()? != ".codeterm" {
        return None;
    }
    codeterm_dir.parent().map(FsPath::to_path_buf)
}

fn load_plan_for_ref<GetPlan, EnsureLoaded>(
    plan_id: &str,
    index_entry: Option<&IndexEntry>,
    mut get_plan: GetPlan,
    mut ensure_loaded: EnsureLoaded,
) -> (Option<crate::plan::Plan>, Option<&'static str>)
where
    GetPlan: FnMut() -> Option<crate::plan::Plan>,
    EnsureLoaded: FnMut(&FsPath) -> Result<(), &'static str>,
{
    if let Some(plan) = get_plan() {
        return (Some(plan), None);
    }
    let Some(entry) = index_entry else {
        return (None, Some("missing"));
    };
    let Some(project_root) = derive_project_root_from_index_path(&entry.file_path, plan_id) else {
        return (None, Some("permission"));
    };
    if let Err(state) = ensure_loaded(&project_root) {
        return (None, Some(state));
    }
    match get_plan() {
        Some(plan) => (Some(plan), None),
        None => (None, Some("missing")),
    }
}

fn find_section<'a>(
    sections: &'a [crate::plan::Section],
    section_id: &str,
) -> Option<&'a crate::plan::Section> {
    for section in sections {
        if section.id == section_id || section.handle.as_deref() == Some(section_id) {
            return Some(section);
        }
        if let Some(found) = find_section(&section.subsections, section_id) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan, PlanStatus};

    fn mk_index_entry(plan_id: &str, file_path: PathBuf, status: PlanStatus) -> IndexEntry {
        IndexEntry {
            plan_id: plan_id.to_string(),
            file_path,
            owner: OrchestratorRef::default(),
            team_id: None,
            title: "Indexed plan".into(),
            status,
            updated_at: 1,
            assigned_pane_ids: vec![],
            task_count: 0,
            open_task_count: 0,
            section_count: 0,
            element_count: 0,
            loop_count: 0,
            has_goal: false,
        }
    }

    fn mk_plan(plan_id: &str) -> Plan {
        let mut plan = Plan::new(
            "Resolved plan".into(),
            "/repo".into(),
            OrchestratorRef::default(),
        );
        plan.id = plan_id.into();
        plan
    }

    #[test]
    fn derive_project_root_accepts_canonical_sidecar_path() {
        let root = derive_project_root_from_index_path(
            FsPath::new("/repo/.codeterm/plans/active/pl_test/plan.yaml"),
            "pl_test",
        );
        assert_eq!(root, Some(PathBuf::from("/repo")));
    }

    #[test]
    fn derive_project_root_rejects_non_sidecar_path() {
        let root = derive_project_root_from_index_path(
            FsPath::new("/repo/.codeterm/plans/active/pl_other/plan.yaml"),
            "pl_test",
        );
        assert!(root.is_none());
    }

    #[test]
    fn load_plan_for_ref_hydrates_cross_project_index_hit() {
        let entry = mk_index_entry(
            "pl_test",
            PathBuf::from("/project-b/.codeterm/plans/active/pl_test/plan.yaml"),
            PlanStatus::Active,
        );
        let loaded = std::cell::RefCell::new(None::<Plan>);
        let (plan, state) = load_plan_for_ref(
            "pl_test",
            Some(&entry),
            || loaded.borrow().clone(),
            |_| {
                *loaded.borrow_mut() = Some(mk_plan("pl_test"));
                Ok(())
            },
        );
        assert!(state.is_none());
        assert_eq!(plan.as_ref().map(|p| p.id.as_str()), Some("pl_test"));
    }

    #[test]
    fn load_plan_for_ref_reports_permission_for_confined_bad_path() {
        let entry = mk_index_entry(
            "pl_test",
            PathBuf::from("/project-b/outside/pl_test.txt"),
            PlanStatus::Active,
        );
        let (plan, state) = load_plan_for_ref("pl_test", Some(&entry), || None, |_| Ok(()));
        assert!(plan.is_none());
        assert_eq!(state, Some("permission"));
    }

    #[test]
    fn load_plan_for_ref_reports_unreachable_from_loader() {
        let entry = mk_index_entry(
            "pl_test",
            PathBuf::from("/project-b/.codeterm/plans/active/pl_test/plan.yaml"),
            PlanStatus::Active,
        );
        let (plan, state) =
            load_plan_for_ref("pl_test", Some(&entry), || None, |_| Err("unreachable"));
        assert!(plan.is_none());
        assert_eq!(state, Some("unreachable"));
    }

    #[test]
    fn load_plan_for_ref_reports_missing_without_index() {
        let (plan, state) = load_plan_for_ref("pl_test", None, || None, |_| Ok(()));
        assert!(plan.is_none());
        assert_eq!(state, Some("missing"));
    }

    #[test]
    fn resolve_ref_marks_archived_unowned_and_stale_targets() {
        let mut archived = mk_plan("pl_archived");
        archived.status = PlanStatus::Archived;
        let archived_entry = mk_index_entry(
            "pl_archived",
            PathBuf::from("/repo/.codeterm/plans/archive/pl_archived/plan.yaml"),
            PlanStatus::Archived,
        );
        let (archived_plan, archived_state) = load_plan_for_ref(
            "pl_archived",
            Some(&archived_entry),
            || Some(archived.clone()),
            |_| Ok(()),
        );
        assert_eq!(archived_state, None);
        assert_eq!(archived_plan.expect("plan").status, PlanStatus::Archived);

        let mut unowned = mk_plan("pl_unowned");
        unowned.owner = OrchestratorRef::default();
        let task_state = resolve_task_state(&unowned, Some("task_missing".into()));
        assert_eq!(task_state, "stale");
    }

    #[test]
    fn transfer_caller_requires_agent_header() {
        let mut headers = HeaderMap::new();
        headers.insert("X-CodeTerm-Pane-Id", "source-manager".parse().unwrap());
        assert_eq!(transfer_caller(&headers).unwrap(), "source-manager");
        let missing = transfer_caller(&HeaderMap::new()).unwrap_err();
        assert_eq!(missing.0, StatusCode::FORBIDDEN);
        assert!(missing.1.contains("X-CodeTerm-Pane-Id is required"));
        assert_eq!(
            team_plan_err(TeamPlanError::Team(
                "Caller pane worker is not authorized to manage Team team_source".into(),
            ))
            .0,
            StatusCode::FORBIDDEN
        );
    }

    fn resolve_task_state(plan: &Plan, target_id: Option<String>) -> &'static str {
        let task = target_id.as_deref().and_then(|task_id| {
            plan.tasks
                .iter()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
        });
        if task.is_some() {
            "resolved"
        } else {
            "stale"
        }
    }
}

pub async fn get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .validate_plan_team_binding(&id)
        .map_err(team_plan_err)?;
    let plan = state
        .svc
        .plan_store
        .get(&id)
        .ok_or_else(|| plan_err(PlanError::NotFound))?;
    Ok(Json(serde_json::to_value(plan).unwrap_or_default()))
}

pub async fn state(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let snapshot = state
        .svc
        .build_plan_state(&id, epoch_ms())
        .map_err(plan_err)?;
    Ok(Json(serde_json::to_value(snapshot).unwrap_or_default()))
}

pub async fn next(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let snapshot = state
        .svc
        .build_plan_state(&id, epoch_ms())
        .map_err(plan_err)?;
    Ok(Json(serde_json::json!({
        "plan_id": id,
        "next_actions": snapshot.next_actions,
    })))
}

#[derive(Deserialize, Default)]
pub(crate) struct ReconcileQuery {
    #[serde(default, deserialize_with = "de_truthy")]
    fix: Option<bool>,
    #[serde(default)]
    caller_pane_id: Option<String>,
}

pub async fn reconcile(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ReconcileQuery>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let report = state
        .svc
        .reconcile_plan(
            &id,
            q.fix == Some(true),
            q.caller_pane_id.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    Ok(Json(serde_json::to_value(report).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
pub(crate) struct UpdatePlanBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    planner: Option<serde_json::Value>,
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
        .update_plan(
            &id,
            body.title.as_deref(),
            status,
            body.mode.as_deref(),
            now,
        )
        .map_err(plan_err)?;
    if let Some(planner_val) = body.planner {
        let planner_ref: Option<PlannerRef> = serde_json::from_value(planner_val)
            .map_err(|e| text_err(StatusCode::UNPROCESSABLE_ENTITY, e))?;
        state
            .svc
            .plan_store
            .set_planner_ref(&id, planner_ref, now)
            .map_err(plan_err)?;
    }
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct ModeBody {
    mode: String,
}

pub async fn mode_set(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ModeBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let mode = serde_json::from_value::<PlanMode>(serde_json::Value::String(body.mode))
        .map_err(|_| plan_err(PlanError::BadMode))?;
    let now = epoch_ms();
    // are lifecycle. Keeps the single execution-task gate authoritative.
    match mode {
        PlanMode::Approved => state
            .svc
            .plan_store
            .set_planning_state(
                &id,
                PlanningState::Approved,
                PlanningAuthor::User,
                None,
                now,
            )
            .map_err(plan_err)?,
        PlanMode::Planning => state
            .svc
            .plan_store
            .set_planning_state(
                &id,
                PlanningState::StructureReady,
                PlanningAuthor::User,
                None,
                now,
            )
            .map_err(plan_err)?,
        PlanMode::Execution | PlanMode::Paused | PlanMode::Complete => state
            .svc
            .plan_store
            .set_mode(&id, mode, now)
            .map_err(plan_err)?,
    }
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
    let mut resp = serde_json::json!({ "ok": true });
    if let Some(plan) = state.svc.plan_store.get(&id) {
        let cleanup = crate::service::plan::cleanup_debt::detect_plan_cleanup_debt(&plan, |pane| {
            state.svc.pane_probe(pane).exists
        });
        push_hints(
            &mut resp,
            crate::service::plan::cleanup_debt::cleanup_debt_hints(&cleanup),
        );
    }
    Ok(Json(resp))
}

pub async fn unarchive(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .unarchive_team_plan(&id, epoch_ms())
        .map_err(team_plan_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct PauseBody {
    reason: String,
    #[serde(default)]
    from: OrchestratorRef,
}

pub async fn pause(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PauseBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let stride_id = state
        .svc
        .plan_store
        .pause_plan(&id, &body.reason, body.from, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(
        serde_json::json!({ "ok": true, "stride_id": stride_id }),
    ))
}

pub async fn resume(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let mode = state
        .svc
        .resume_team_plan(&id, epoch_ms())
        .map_err(team_plan_err)?;
    Ok(Json(serde_json::json!({ "ok": true, "mode": mode })))
}

#[derive(Deserialize)]
pub(crate) struct TransferBody {
    to: OrchestratorRef,
    #[serde(default)]
    reason: String,
}

fn transfer_caller(headers: &HeaderMap) -> Result<String, (StatusCode, Json<Value>)> {
    crate::api::pane_routes::caller_pane_header(headers)
        .filter(|pane_id| !pane_id.trim().is_empty())
        .ok_or_else(|| {
            text_err(
                StatusCode::FORBIDDEN,
                "X-CodeTerm-Pane-Id is required for plan transfer",
            )
        })
}

pub async fn transfer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TransferBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let caller = transfer_caller(&headers)?;
    state
        .svc
        .transfer_team_plan(&id, body.to, &body.reason, caller, epoch_ms())
        .map_err(team_plan_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

fn team_plan_err(error: TeamPlanError) -> (StatusCode, Json<Value>) {
    match error {
        TeamPlanError::Plan(error) => plan_err(error),
        TeamPlanError::Team(message)
            if message.contains("not authorized") || message.contains("not a member") =>
        {
            text_err(StatusCode::FORBIDDEN, message)
        }
        TeamPlanError::Team(message) => text_err(StatusCode::BAD_REQUEST, message),
    }
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

#[derive(Deserialize)]
pub(crate) struct TagFacetsSetBody {
    facets: Value,
}

pub async fn tag_facets_set(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TagFacetsSetBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let facets = tag_facets_from_body(&body.facets)
        .map_err(|message| text_err(StatusCode::BAD_REQUEST, message))?;
    state
        .svc
        .plan_store
        .set_tag_facets(&id, facets, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}
