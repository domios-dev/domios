pub mod body_files;
pub mod mutations;
pub mod paths;
pub mod reminders;
pub mod task_queries;
pub mod watcher;

#[cfg(test)]
mod tests;

use crate::plan::{OrchestratorRef, Plan, PlanStatus, INBOX_PLAN_FILE_STEM, INBOX_PLAN_ID};
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

#[derive(Default)]
pub struct PlanStore {
    plans: RwLock<HashMap<String, Arc<Mutex<Plan>>>>,
    index: RwLock<HashMap<String, IndexEntry>>,
    last_writes: RwLock<HashMap<PathBuf, SystemTime>>,
    turn_counter: RwLock<Option<Arc<reminders::TurnCounter>>>,
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
        body_files::persist_bodies(plan)?;
        let json_plan = body_files::strip_bodies(plan);
        let json = serde_json::to_string_pretty(&json_plan)
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

    fn load_file_into_index(&self, path: PathBuf, project_root: &Path) -> Result<(), PlanError> {
        let json = std::fs::read_to_string(&path).map_err(|e| PlanError::Io(e.to_string()))?;
        let mut plan: Plan =
            serde_json::from_str(&json).map_err(|e| PlanError::Io(e.to_string()))?;
        plan.project_path = project_root.display().to_string();
        let migrated =
            body_files::hydrate_and_migrate(&mut plan).map_err(|e| PlanError::Io(e.to_string()))?;
        if migrated {
            let path = Self::write_atomic(&plan).map_err(|e| PlanError::Io(e.to_string()))?;
            self.note_plan_write(&plan, &path);
        }
        let entry = Self::index_entry(&plan, path);
        self.index.write().insert(plan.id.clone(), entry);
        self.plans
            .write()
            .insert(plan.id.clone(), Arc::new(Mutex::new(plan)));
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
        Ok(project_root)
    }

    pub fn save(&self, plan: &mut Plan, now_ms: u64) -> std::io::Result<()> {
        if plan.created_at == 0 {
            plan.created_at = now_ms;
        }
        plan.updated_at = now_ms;
        let path = Self::write_atomic(plan)?;
        self.note_plan_write(plan, &path);
        self.index
            .write()
            .insert(plan.id.clone(), Self::index_entry(plan, path));
        self.plans
            .write()
            .insert(plan.id.clone(), Arc::new(Mutex::new(plan.clone())));
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<Plan> {
        self.plans.read().get(id).map(|p| p.lock().clone())
    }

    pub fn coverage_report(&self, id: &str) -> Option<crate::plan::coverage::CoverageReport> {
        self.get(id).and_then(|plan| {
            (plan.status != PlanStatus::Archived)
                .then(|| crate::plan::coverage::compute_coverage_report(&plan))
        })
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
        if migrated {
            let path = Self::write_atomic(&plan).map_err(|e| PlanError::Io(e.to_string()))?;
            self.note_plan_write(&plan, &path);
        }
        self.index
            .write()
            .insert(plan.id.clone(), Self::index_entry(&plan, path));
        self.plans
            .write()
            .insert(plan.id.clone(), Arc::new(Mutex::new(plan)));
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
            let _ = body_files::remove_body_dir(&plan);
        }
        Ok(())
    }
}
