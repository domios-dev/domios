pub mod batch;
pub mod body_files;
pub mod handoff;
pub mod hints;
pub mod mutations;
pub mod paths;
pub mod reminders;
pub mod search_index;
pub mod spec;
pub mod spec_files;
pub mod tag_model;
pub mod task_queries;
pub mod watcher;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod timestamp_tests;

use crate::plan::{OrchestratorRef, Plan, PlanStatus, Spec, INBOX_PLAN_FILE_STEM, INBOX_PLAN_ID};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Debug)]
pub enum PlanError {
    NotFound,
    TaskNotFound,
    UndeclaredKind { declared: Vec<String> },
    BudgetExhausted { count: u32, budget: u32 },
    BadStatus,
    BadPriority,
    Io(String),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::NotFound => write!(f, "plan not found"),
            PlanError::TaskNotFound => write!(f, "task not found"),
            PlanError::UndeclaredKind { declared } => {
                write!(
                    f,
                    "undeclared milestone kind; declared: {}",
                    declared.join(", ")
                )
            }
            PlanError::BudgetExhausted { count, budget } => {
                write!(f, "loop budget {count}/{budget} reached")
            }
            PlanError::BadStatus => write!(f, "invalid status"),
            PlanError::BadPriority => write!(f, "invalid priority"),
            PlanError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for PlanError {}

impl PlanError {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            PlanError::NotFound => serde_json::json!({ "type": "not_found" }),
            PlanError::TaskNotFound => serde_json::json!({ "type": "task_not_found" }),
            PlanError::UndeclaredKind { declared } => {
                serde_json::json!({ "type": "undeclared_kind", "declared": declared })
            }
            PlanError::BudgetExhausted { count, budget } => {
                serde_json::json!({ "type": "budget_exhausted", "count": count, "budget": budget })
            }
            PlanError::BadStatus => serde_json::json!({ "type": "bad_status" }),
            PlanError::BadPriority => serde_json::json!({ "type": "bad_priority" }),
            PlanError::Io(message) => serde_json::json!({ "type": "io", "message": message }),
        }
    }
}

#[derive(Default)]
pub struct NewTask {
    pub name: String,
    pub kind: String,
    pub section_ids: Vec<String>,
    pub model: Option<String>,
    pub model_rationale: Option<String>,
    pub body: String,
    pub intent: Option<String>,
    pub parent_task_id: Option<String>,
    pub assigned_pane_id: Option<String>,
    pub group: Option<String>,
    pub priority: Option<crate::plan::TaskPriority>,
    pub handle: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexEntry {
    pub plan_id: String,
    pub file_path: PathBuf,
    pub owner: OrchestratorRef,
    pub title: String,
    pub status: PlanStatus,
    pub updated_at: u64,
    pub assigned_pane_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpecIndexEntry {
    pub spec_id: String,
    pub file_path: PathBuf,
    pub title: String,
    pub status: PlanStatus,
    pub updated_at: u64,
    pub project_path: String,
}

#[derive(Default)]
pub struct PlanStore {
    plans: RwLock<HashMap<String, Arc<Mutex<Plan>>>>,
    specs: RwLock<HashMap<String, Arc<Mutex<Spec>>>>,
    index: RwLock<HashMap<String, IndexEntry>>,
    spec_index: RwLock<HashMap<String, SpecIndexEntry>>,
    last_writes: RwLock<HashMap<PathBuf, SystemTime>>,
    turn_counter: RwLock<Option<Arc<reminders::TurnCounter>>>,
    reseed_prepare_lock: Mutex<()>,
}

impl PlanStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_for_test(_root: &Path) -> Self {
        Self::default()
    }

    fn file_path(plan: &Plan) -> std::io::Result<PathBuf> {
        let dir = paths::plans_dir(Path::new(&plan.project_path))?;
        let stem = if plan.id == INBOX_PLAN_ID {
            INBOX_PLAN_FILE_STEM
        } else {
            &plan.id
        };
        Ok(dir.join(format!("{stem}.json")))
    }

    fn write_atomic(plan: &Plan) -> std::io::Result<PathBuf> {
        let path = Self::file_path(plan)?;
        let tmp = path.with_extension("json.tmp");
        let mut normalized = plan.clone();
        tag_model::normalize_plan(&mut normalized);
        body_files::persist_bodies(&normalized)?;
        let json_plan = body_files::strip_bodies(&normalized);
        let json = serde_json::to_string_pretty(&json_plan)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }

    fn spec_file_path(spec: &Spec) -> std::io::Result<PathBuf> {
        Ok(paths::specs_dir(Path::new(&spec.project_path))?.join(format!("{}.json", spec.id)))
    }

    fn write_spec_atomic(spec: &Spec) -> std::io::Result<PathBuf> {
        let path = Self::spec_file_path(spec)?;
        let tmp = path.with_extension("json.tmp");
        let mut normalized = spec.clone();
        tag_model::normalize_sections_for_spec(&mut normalized.sections);
        spec_files::persist_bodies(&normalized)?;
        let json_spec = spec_files::strip_bodies(&normalized);
        let json = serde_json::to_string_pretty(&json_spec)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }

    fn note_write(&self, path: &Path) {
        if let Ok(meta) = std::fs::metadata(path) {
            if let Ok(modified) = meta.modified() {
                self.last_writes
                    .write()
                    .insert(path.to_path_buf(), modified);
            }
        }
    }

    fn note_plan_write(&self, plan: &Plan, path: &Path) {
        self.note_write(path);
        if let Ok(body_paths) = body_files::body_paths(plan) {
            for path in body_paths {
                self.note_write(&path);
            }
        }
    }

    fn note_spec_write(&self, spec: &Spec, path: &Path) {
        self.note_write(path);
        if let Ok(body_paths) = spec_files::body_paths(spec) {
            for path in body_paths {
                self.note_write(&path);
            }
        }
    }

    pub fn is_own_recent_write(&self, path: &Path) -> bool {
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        let Ok(modified) = meta.modified() else {
            return false;
        };
        self.last_writes
            .read()
            .get(path)
            .is_some_and(|last| *last == modified)
    }

    fn index_entry(plan: &Plan, file_path: PathBuf) -> IndexEntry {
        IndexEntry {
            plan_id: plan.id.clone(),
            file_path,
            owner: plan.owner.clone(),
            title: plan.title.clone(),
            status: plan.status.clone(),
            updated_at: plan.updated_at,
            assigned_pane_ids: plan
                .tasks
                .iter()
                .filter_map(|t| t.assigned_pane_id.clone())
                .collect(),
        }
    }

    fn spec_index_entry(spec: &Spec, file_path: PathBuf) -> SpecIndexEntry {
        SpecIndexEntry {
            spec_id: spec.id.clone(),
            file_path,
            title: spec.title.clone(),
            status: spec.status.clone(),
            updated_at: spec.updated_at,
            project_path: spec.project_path.clone(),
        }
    }

    fn upsert_plan_arc(&self, plan: Plan) -> Plan {
        let id = plan.id.clone();
        let existing = self.plans.read().get(&id).cloned();
        if let Some(arc) = existing {
            let Some(mut guard) = arc.try_lock() else {
                let mut guard = arc.lock();
                if guard.updated_at >= plan.updated_at {
                    return guard.clone();
                }
                *guard = plan;
                return guard.clone();
            };
            if guard.updated_at > plan.updated_at {
                return guard.clone();
            }
            *guard = plan;
            guard.clone()
        } else {
            let mut plans = self.plans.write();
            if let Some(arc) = plans.get(&id).cloned() {
                drop(plans);
                let Some(mut guard) = arc.try_lock() else {
                    let mut guard = arc.lock();
                    if guard.updated_at >= plan.updated_at {
                        return guard.clone();
                    }
                    *guard = plan;
                    return guard.clone();
                };
                if guard.updated_at > plan.updated_at {
                    return guard.clone();
                }
                *guard = plan;
                guard.clone()
            } else {
                plans.insert(id, Arc::new(Mutex::new(plan.clone())));
                plan
            }
        }
    }

    fn upsert_spec_arc(&self, spec: Spec) -> Spec {
        let id = spec.id.clone();
        let existing = self.specs.read().get(&id).cloned();
        if let Some(arc) = existing {
            let Some(mut guard) = arc.try_lock() else {
                let mut guard = arc.lock();
                if guard.updated_at >= spec.updated_at {
                    return guard.clone();
                }
                *guard = spec;
                return guard.clone();
            };
            if guard.updated_at > spec.updated_at {
                return guard.clone();
            }
            *guard = spec;
            guard.clone()
        } else {
            let mut specs = self.specs.write();
            if let Some(arc) = specs.get(&id).cloned() {
                drop(specs);
                let Some(mut guard) = arc.try_lock() else {
                    let mut guard = arc.lock();
                    if guard.updated_at >= spec.updated_at {
                        return guard.clone();
                    }
                    *guard = spec;
                    return guard.clone();
                };
                if guard.updated_at > spec.updated_at {
                    return guard.clone();
                }
                *guard = spec;
                guard.clone()
            } else {
                specs.insert(id, Arc::new(Mutex::new(spec.clone())));
                spec
            }
        }
    }

    pub(super) fn commit_loaded_plan(&self, plan: Plan) -> Result<(), PlanError> {
        let committed = self.upsert_plan_arc(plan);
        self.mirror_plan_index(&committed)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        let path = Self::write_atomic(&committed).map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_plan_write(&committed, &path);
        let entry = Self::index_entry(&committed, path);
        self.index.write().insert(committed.id.clone(), entry);
        self.reset_bound_panes(&committed);
        Ok(())
    }

    pub(super) fn commit_loaded_spec(&self, spec: Spec) -> Result<(), PlanError> {
        let committed = self.upsert_spec_arc(spec);
        self.mirror_spec_index(&committed)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        let path = Self::write_spec_atomic(&committed).map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_spec_write(&committed, &path);
        let entry = Self::spec_index_entry(&committed, path);
        self.spec_index.write().insert(committed.id.clone(), entry);
        Ok(())
    }

    fn load_file_into_index(&self, path: PathBuf, project_root: &Path) -> Result<(), PlanError> {
        let json = std::fs::read_to_string(&path).map_err(|e| PlanError::Io(e.to_string()))?;
        let mut plan: Plan =
            serde_json::from_str(&json).map_err(|e| PlanError::Io(e.to_string()))?;
        plan.project_path = project_root.display().to_string();
        let migrated =
            body_files::hydrate_and_migrate(&mut plan).map_err(|e| PlanError::Io(e.to_string()))?;
        tag_model::normalize_plan(&mut plan);
        if migrated {
            let path = Self::write_atomic(&plan).map_err(|e| PlanError::Io(e.to_string()))?;
            self.note_plan_write(&plan, &path);
        }
        let committed = self.upsert_plan_arc(plan);
        self.mirror_plan_index(&committed)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_plan_write(&committed, &path);
        let entry = Self::index_entry(&committed, path);
        self.index.write().insert(committed.id.clone(), entry);
        Ok(())
    }

    fn load_spec_file_into_index(
        &self,
        path: PathBuf,
        project_root: &Path,
    ) -> Result<(), PlanError> {
        let json = std::fs::read_to_string(&path).map_err(|e| PlanError::Io(e.to_string()))?;
        let mut spec: Spec =
            serde_json::from_str(&json).map_err(|e| PlanError::Io(e.to_string()))?;
        spec.project_path = project_root.display().to_string();
        let migrated =
            spec_files::hydrate_and_migrate(&mut spec).map_err(|e| PlanError::Io(e.to_string()))?;
        if migrated {
            let path = Self::write_spec_atomic(&spec).map_err(|e| PlanError::Io(e.to_string()))?;
            self.note_spec_write(&spec, &path);
        }
        let committed = self.upsert_spec_arc(spec);
        self.mirror_spec_index(&committed)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_spec_write(&committed, &path);
        let entry = Self::spec_index_entry(&committed, path);
        self.spec_index.write().insert(committed.id.clone(), entry);
        Ok(())
    }

    pub fn scan_project(&self, project_path: &Path) -> Result<PathBuf, PlanError> {
        let project_root = paths::project_root(project_path);
        let plans_dir =
            paths::plans_dir(&project_root).map_err(|e| PlanError::Io(e.to_string()))?;
        let entries = std::fs::read_dir(&plans_dir).map_err(|e| PlanError::Io(e.to_string()))?;
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Err(e) = self.load_file_into_index(path.clone(), &project_root) {
                log::warn!("[plan] skip malformed plan {}: {}", path.display(), e);
            }
        }
        let specs_dir =
            paths::specs_dir(&project_root).map_err(|e| PlanError::Io(e.to_string()))?;
        if let Ok(entries) = std::fs::read_dir(&specs_dir) {
            for entry in entries {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Err(e) = self.load_spec_file_into_index(path.clone(), &project_root) {
                    log::warn!("[spec] skip malformed spec {}: {}", path.display(), e);
                }
            }
        }
        Ok(project_root)
    }

    pub fn save(&self, plan: &mut Plan, now_ms: u64) -> std::io::Result<()> {
        if plan.created_at == 0 {
            plan.created_at = now_ms;
        }
        plan.updated_at = now_ms;
        tag_model::normalize_plan(plan);
        let id = plan.id.clone();
        let arc = if let Some(arc) = self.plans.read().get(&id).cloned() {
            arc
        } else {
            let mut plans = self.plans.write();
            plans
                .entry(id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(plan.clone())))
                .clone()
        };
        let mut guard = arc.lock();
        if guard.updated_at <= plan.updated_at {
            *guard = plan.clone();
        }
        *plan = guard.clone();
        let path = Self::write_atomic(&guard)?;
        self.note_plan_write(&guard, &path);
        self.mirror_plan_index(&guard)
            .map_err(std::io::Error::other)?;
        self.index
            .write()
            .insert(guard.id.clone(), Self::index_entry(&guard, path));
        Ok(())
    }

    pub fn save_spec(&self, spec: &mut Spec, now_ms: u64) -> std::io::Result<()> {
        if spec.created_at == 0 {
            spec.created_at = now_ms;
        }
        spec.updated_at = now_ms;
        tag_model::normalize_sections_for_spec(&mut spec.sections);
        let id = spec.id.clone();
        let arc = if let Some(arc) = self.specs.read().get(&id).cloned() {
            arc
        } else {
            let mut specs = self.specs.write();
            specs
                .entry(id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(spec.clone())))
                .clone()
        };
        let mut guard = arc.lock();
        if guard.updated_at <= spec.updated_at {
            *guard = spec.clone();
        }
        *spec = guard.clone();
        let path = Self::write_spec_atomic(&guard)?;
        self.note_spec_write(&guard, &path);
        self.mirror_spec_index(&guard)
            .map_err(std::io::Error::other)?;
        self.spec_index
            .write()
            .insert(guard.id.clone(), Self::spec_index_entry(&guard, path));
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<Plan> {
        self.plans.read().get(id).map(|p| p.lock().clone())
    }

    pub fn get_spec(&self, id: &str) -> Option<Spec> {
        self.specs.read().get(id).map(|s| s.lock().clone())
    }

    pub fn coverage_report(&self, id: &str) -> Option<crate::plan::coverage::CoverageReport> {
        self.get(id).and_then(|plan| {
            (plan.status != PlanStatus::Archived)
                .then(|| crate::plan::coverage::compute_coverage_report(&plan))
        })
    }

    pub fn spec_coverage_report(
        &self,
        id: &str,
    ) -> Option<crate::plan::coverage::SpecCoverageReport> {
        let spec = self.get_spec(id)?;
        let plans = self.list_full();
        Some(crate::plan::coverage::compute_spec_coverage_report(
            &spec, &plans,
        ))
    }

    pub fn spec_full(&self, id: &str) -> Option<serde_json::Value> {
        let spec = self.get_spec(id)?;
        let coverage = self.spec_coverage_report(id)?;
        let links = self.list_spec_links(id);
        Some(serde_json::json!({
            "spec": spec,
            "coverage": coverage,
            "links": links,
        }))
    }

    pub fn effort_bundle(&self, plan_id: &str) -> Option<serde_json::Value> {
        let plan = self.get(plan_id)?;
        let specs = plan
            .spec_refs
            .iter()
            .filter_map(|id| self.get_spec(id))
            .collect::<Vec<_>>();
        let spec_coverage = specs
            .iter()
            .map(|spec| {
                serde_json::json!({
                    "spec_id": spec.id,
                    "coverage": self.spec_coverage_report(&spec.id),
                })
            })
            .collect::<Vec<_>>();
        Some(serde_json::json!({
            "plan": plan,
            "specs": specs,
            "spec_coverage": spec_coverage,
        }))
    }

    pub fn spec_markdown(&self, id: &str) -> Option<String> {
        self.get(id).map(|plan| body_files::spec_markdown(&plan))
    }

    pub fn file_manifest(&self, id: &str) -> Result<body_files::PlanFileManifest, PlanError> {
        let plan = self.get(id).ok_or(PlanError::NotFound)?;
        body_files::file_manifest(&plan).map_err(|e| PlanError::Io(e.to_string()))
    }

    pub fn spec_file_manifest(&self, id: &str) -> Result<spec_files::SpecFileManifest, PlanError> {
        let spec = self.get_spec(id).ok_or(PlanError::NotFound)?;
        spec_files::file_manifest(&spec).map_err(|e| PlanError::Io(e.to_string()))
    }

    pub fn spec_document_markdown(&self, id: &str) -> Option<String> {
        self.get_spec(id)
            .map(|spec| spec_files::spec_markdown(&spec))
    }

    pub fn reload_from_disk(&self, id: &str) -> Result<(), PlanError> {
        let path = self
            .index
            .read()
            .get(id)
            .map(|e| e.file_path.clone())
            .ok_or(PlanError::NotFound)?;
        let json = std::fs::read_to_string(&path).map_err(|e| PlanError::Io(e.to_string()))?;
        let mut plan: Plan =
            serde_json::from_str(&json).map_err(|e| PlanError::Io(e.to_string()))?;
        if plan.id != id {
            return Err(PlanError::Io(format!(
                "plan id mismatch: expected {id}, found {}",
                plan.id
            )));
        }
        let project_root = paths::project_root(Path::new(&plan.project_path));
        plan.project_path = project_root.display().to_string();
        let migrated =
            body_files::hydrate_and_migrate(&mut plan).map_err(|e| PlanError::Io(e.to_string()))?;
        tag_model::normalize_plan(&mut plan);
        if migrated {
            let path = Self::write_atomic(&plan).map_err(|e| PlanError::Io(e.to_string()))?;
            self.note_plan_write(&plan, &path);
        }
        let committed = self.upsert_plan_arc(plan);
        self.mirror_plan_index(&committed)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_plan_write(&committed, &path);
        self.index
            .write()
            .insert(committed.id.clone(), Self::index_entry(&committed, path));
        Ok(())
    }

    pub fn bind_turn_counter(&self, counter: Arc<reminders::TurnCounter>) {
        *self.turn_counter.write() = Some(counter);
    }

    fn reset_bound_panes(&self, plan: &Plan) {
        let Some(counter) = self.turn_counter.read().clone() else {
            return;
        };
        for pane_id in plan
            .tasks
            .iter()
            .filter_map(|t| t.assigned_pane_id.as_deref())
        {
            counter.reset(pane_id);
        }
    }

    pub fn task_binding_for_pane(&self, pane_id: &str) -> Option<(String, String)> {
        for plan in self.plans.read().values() {
            let plan = plan.lock();
            if plan.status != PlanStatus::Active {
                continue;
            }
            if let Some(task) = plan
                .tasks
                .iter()
                .find(|t| t.assigned_pane_id.as_deref() == Some(pane_id))
            {
                return Some((plan.id.clone(), task.id.clone()));
            }
        }
        None
    }

    pub fn list_index(&self) -> Vec<IndexEntry> {
        self.index.read().values().cloned().collect()
    }

    pub fn list_spec_index(&self) -> Vec<SpecIndexEntry> {
        self.spec_index.read().values().cloned().collect()
    }

    pub fn list_full(&self) -> Vec<Plan> {
        self.plans
            .read()
            .values()
            .map(|p| p.lock().clone())
            .collect()
    }

    pub fn delete(&self, id: &str) -> std::io::Result<()> {
        let plan = self
            .plans
            .write()
            .remove(id)
            .map(|plan| plan.lock().clone());
        if let Some(entry) = self.index.write().remove(id) {
            let _ = std::fs::remove_file(&entry.file_path);
        }
        if let Some(plan) = plan {
            let _ = search_index::delete_plan_units(&plan);
            let _ = body_files::remove_body_dir(&plan);
        }
        Ok(())
    }
}
