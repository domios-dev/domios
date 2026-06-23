pub mod mutations;
pub mod paths;
pub mod reminders;
pub mod watcher;

#[cfg(test)]
mod tests;

use crate::plan::{OrchestratorRef, Plan, PlanStatus};
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
    pub assigned_pane_id: Option<String>,
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
        Ok(dir.join(format!("{}.json", plan.id)))
    }

    fn write_atomic(plan: &Plan) -> std::io::Result<PathBuf> {
        let path = Self::file_path(plan)?;
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(plan)
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

    pub fn save(&self, plan: &mut Plan, now_ms: u64) -> std::io::Result<()> {
        if plan.created_at == 0 {
            plan.created_at = now_ms;
        }
        plan.updated_at = now_ms;
        let path = Self::write_atomic(plan)?;
        self.note_write(&path);
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

    pub fn reload_from_disk(&self, id: &str) -> Result<(), PlanError> {
        let path = self
            .index
            .read()
            .get(id)
            .map(|e| e.file_path.clone())
            .ok_or(PlanError::NotFound)?;
        let json = std::fs::read_to_string(&path).map_err(|e| PlanError::Io(e.to_string()))?;
        let plan: Plan = serde_json::from_str(&json).map_err(|e| PlanError::Io(e.to_string()))?;
        if plan.id != id {
            return Err(PlanError::Io(format!(
                "plan id mismatch: expected {id}, found {}",
                plan.id
            )));
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
        self.plans.read().values().map(|p| p.lock().clone()).collect()
    }

    pub fn delete(&self, id: &str) -> std::io::Result<()> {
        if let Some(entry) = self.index.write().remove(id) {
            let _ = std::fs::remove_file(&entry.file_path);
        }
        self.plans.write().remove(id);
        Ok(())
    }
}
