use serde::Serialize;
use std::path::{Path, PathBuf};

use super::{body_files, paths, store_v2, PlanError, PlanStore};
use crate::plan::{OrchestratorRef, Plan, PlanStatus, PlannerRef};

#[derive(Debug, Clone, Default)]
pub struct AdoptPlanRequest {
    pub source: PathBuf,
    pub target_project: PathBuf,
    pub owner: OrchestratorRef,
    pub dry_run: bool,
    pub overwrite: bool,
    pub remove_source: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AdoptPlanReport {
    pub ok: bool,
    pub dry_run: bool,
    pub moved: bool,
    pub plan_id: String,
    pub source_dir: String,
    pub target_dir: String,
    pub target_project: String,
    pub task_count: usize,
    pub warnings: Vec<String>,
}

pub trait AdoptPlanExt {
    fn adopt_plan(
        &self,
        request: AdoptPlanRequest,
        now_ms: u64,
    ) -> Result<AdoptPlanReport, PlanError>;
}

impl AdoptPlanExt for PlanStore {
    fn adopt_plan(
        &self,
        request: AdoptPlanRequest,
        now_ms: u64,
    ) -> Result<AdoptPlanReport, PlanError> {
        let source_dir = source_plan_dir(&request.source)?;
        let mut plan = load_source_plan(&source_dir)?;
        let target_project = paths::project_root(&request.target_project);
        plan.status = PlanStatus::Archived;
        plan.team_id = None;
        let target_dir = plan_dir_unchecked(&target_project, &plan.status, &plan.id);
        let task_count = canonical_task_count(&source_dir, &plan);
        let warnings = adoption_warnings(
            &plan,
            &source_dir,
            &target_project,
            self,
            &request,
            task_count,
        );
        let report = AdoptPlanReport {
            ok: request.overwrite || !destination_exists(&target_dir, &plan.id),
            dry_run: request.dry_run,
            moved: request.remove_source && !request.dry_run,
            plan_id: plan.id.clone(),
            source_dir: source_dir.display().to_string(),
            target_dir: target_dir.display().to_string(),
            target_project: target_project.display().to_string(),
            task_count,
            warnings,
        };
        if request.dry_run {
            return Ok(report);
        }
        if !request.overwrite && destination_exists(&target_dir, &plan.id) {
            return Err(PlanError::Io(format!(
                "destination plan '{}' already exists at {}; rerun with --overwrite",
                plan.id,
                target_dir.display()
            )));
        }
        if target_dir.exists() {
            std::fs::remove_dir_all(&target_dir).map_err(|e| PlanError::Io(e.to_string()))?;
        }
        copy_dir_all(&source_dir, &target_dir).map_err(|e| PlanError::Io(e.to_string()))?;
        normalize_adopted_plan(&mut plan, &target_project, request.owner);
        self.save(&mut plan, now_ms)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        self.reload_from_disk(&plan.id)?;
        if request.remove_source && source_dir != target_dir && source_dir.exists() {
            std::fs::remove_dir_all(&source_dir).map_err(|e| PlanError::Io(e.to_string()))?;
        }
        Ok(report)
    }
}

fn source_plan_dir(source: &Path) -> Result<PathBuf, PlanError> {
    let source = source
        .canonicalize()
        .map_err(|e| PlanError::Io(format!("resolve source plan: {e}")))?;
    if source.is_dir() && (source.join("plan.yaml").is_file() || source.join("plan.json").is_file())
    {
        return Ok(source);
    }
    if matches!(
        source.file_name().and_then(|name| name.to_str()),
        Some("plan.yaml" | "plan.json")
    ) {
        return source
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| PlanError::Io("source plan.yaml has no parent directory".into()));
    }
    Err(PlanError::Io(format!(
        "source must be a plan sidecar directory or plan.yaml: {}",
        source.display()
    )))
}

fn load_source_plan(source_dir: &Path) -> Result<Plan, PlanError> {
    if source_dir.join("plan.yaml").is_file() {
        let plan = store_v2::load_plan(source_dir).map_err(|e| PlanError::Io(e.to_string()))?;
        let task_count = canonical_task_count(source_dir, &plan);
        if task_count != plan.tasks.len() {
            return Err(PlanError::Io(format!(
                "source plan has {task_count} task sidecar file(s), but only {} loaded",
                plan.tasks.len()
            )));
        }
        return Ok(plan);
    }
    let json_path = source_dir.join("plan.json");
    let json = std::fs::read_to_string(&json_path).map_err(|e| PlanError::Io(e.to_string()))?;
    let mut plan: Plan = serde_json::from_str(&json).map_err(|e| PlanError::Io(e.to_string()))?;
    body_files::hydrate_and_migrate(&mut plan).map_err(|e| PlanError::Io(e.to_string()))?;
    Ok(plan)
}

fn plan_dir_unchecked(project: &Path, status: &PlanStatus, plan_id: &str) -> PathBuf {
    project
        .join(".codeterm")
        .join("plans")
        .join(paths::plan_bucket(status))
        .join(if plan_id == crate::plan::INBOX_PLAN_ID {
            crate::plan::INBOX_PLAN_FILE_STEM
        } else {
            plan_id
        })
}

fn destination_exists(target_dir: &Path, plan_id: &str) -> bool {
    target_dir.join("plan.yaml").is_file()
        || target_dir.join("plan.json").is_file()
        || target_dir
            .parent()
            .and_then(|bucket| bucket.parent())
            .is_some_and(|plans_dir| {
                ["active", "archive"].iter().any(|bucket| {
                    let dir =
                        plans_dir
                            .join(bucket)
                            .join(if plan_id == crate::plan::INBOX_PLAN_ID {
                                crate::plan::INBOX_PLAN_FILE_STEM
                            } else {
                                plan_id
                            });
                    dir.join("plan.yaml").is_file() || dir.join("plan.json").is_file()
                })
            })
}

fn canonical_task_count(source_dir: &Path, plan: &Plan) -> usize {
    let tasks_dir = source_dir.join("tasks");
    let sidecar_count = std::fs::read_dir(&tasks_dir)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    entry.path().extension().and_then(|ext| ext.to_str()) == Some("yaml")
                })
                .count()
        })
        .unwrap_or(0);
    sidecar_count.max(plan.tasks.len())
}

fn adoption_warnings(
    plan: &Plan,
    source_dir: &Path,
    target_project: &Path,
    store: &PlanStore,
    request: &AdoptPlanRequest,
    task_count: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();
    warnings.push(format!(
        "plan will remain archived until adopted by a live Team: codeterm team adopt-plan <team-id> {}",
        plan.id
    ));
    let source_project = paths::project_root(Path::new(
        if plan.canonical_project_path.trim().is_empty() {
            &plan.project_path
        } else {
            &plan.canonical_project_path
        },
    ));
    if source_project != target_project {
        warnings.push(format!(
            "source project {} differs from target project {}",
            source_project.display(),
            target_project.display()
        ));
    }
    if plan.owner.pane_id.is_some() || plan.owner.session_id.is_some() {
        warnings.push("source owner binding will be replaced by the caller".into());
    }
    if matches!(plan.planner, Some(PlannerRef::Pane { .. })) {
        warnings.push("source planner Tab binding will be cleared".into());
    }
    if task_count != plan.tasks.len() {
        warnings.push(format!(
            "source sidecar task count is {task_count} but the loaded plan body contains {} tasks",
            plan.tasks.len()
        ));
    }
    let stale_tasks = plan
        .tasks
        .iter()
        .filter(|task| {
            task.assigned_pane_id.is_some()
                || task.lane.pane_id.is_some()
                || task.lane.workspace_id.is_some()
                || task.lane.worktree_path.is_some()
        })
        .count();
    if stale_tasks > 0 {
        warnings.push(format!(
            "{stale_tasks} task Tab/lane bindings will be cleared as stale"
        ));
    }
    if plan.heartbeat.current_focus.is_some() || !plan.heartbeat.active_lanes.is_empty() {
        warnings.push("heartbeat focus and active lanes will be cleared".into());
    }
    let target_dir = plan_dir_unchecked(target_project, &plan.status, &plan.id);
    if destination_exists(&target_dir, &plan.id) || store.get(&plan.id).is_some() {
        warnings.push(format!("destination plan id {} already exists", plan.id));
    }
    if request.remove_source && source_dir.starts_with(target_project) {
        warnings.push("source sidecar is under the target project; move would remove the source after adoption".into());
    }
    warnings
}

fn normalize_adopted_plan(plan: &mut Plan, target_project: &Path, owner: OrchestratorRef) {
    let project = target_project.display().to_string();
    plan.project_path = project.clone();
    plan.canonical_project_path = project.clone();
    plan.owner = owner;
    plan.planner = None;
    plan.heartbeat.current_focus = None;
    plan.heartbeat.active_lanes.clear();
    for task in &mut plan.tasks {
        task.assigned_pane_id = None;
        task.taken_by_role = None;
        task.peer = None;
        task.lane.pane_id = None;
        task.lane.workspace_id = None;
        task.lane.workspace_name = None;
        task.lane.worktree_path = None;
    }
}

fn copy_dir_all(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target_path = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target_path)?;
        } else {
            std::fs::copy(entry.path(), target_path)?;
        }
    }
    Ok(())
}
