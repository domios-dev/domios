pub mod mutations;
pub mod paths;

#[cfg(test)]
mod tests;

use crate::plan::{OrchestratorRef, Plan, PlanStatus};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
}

#[derive(Default)]
pub struct PlanStore {
    plans: RwLock<HashMap<String, Arc<Mutex<Plan>>>>,
    index: RwLock<HashMap<String, IndexEntry>>,
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

    fn index_entry(plan: &Plan, file_path: PathBuf) -> IndexEntry {
        IndexEntry {
            plan_id: plan.id.clone(),
            file_path,
            owner: plan.owner.clone(),
            title: plan.title.clone(),
            status: plan.status.clone(),
            updated_at: plan.updated_at,
        }
    }

    pub fn save(&self, plan: &mut Plan, now_ms: u64) -> std::io::Result<()> {
        if plan.created_at == 0 {
            plan.created_at = now_ms;
        }
        plan.updated_at = now_ms;
        let path = Self::write_atomic(plan)?;
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

    pub fn list_index(&self) -> Vec<IndexEntry> {
        self.index.read().values().cloned().collect()
    }

    pub fn delete(&self, id: &str) -> std::io::Result<()> {
        if let Some(entry) = self.index.write().remove(id) {
            let _ = std::fs::remove_file(&entry.file_path);
        }
        self.plans.write().remove(id);
        Ok(())
    }
}
