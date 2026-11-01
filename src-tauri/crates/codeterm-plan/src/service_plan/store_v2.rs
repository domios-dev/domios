//! Plan storage v2 — one YAML file per entity, the directory IS the graph.
//!
//! Layout per plan dir:
//!   plan.yaml            plan-own facts + sections (tasks/goal stripped out)
//!   goal.yaml            the Goal
//!   tasks/<handle>.yaml  one complete Task (body + milestones inline)
//!   tasks/<handle>.jsonl strides/activity log (unchanged, append-only)
//!
//! No plan.json, no frontmatter, no per-entity fan-out: a mutation writes
//! only the entity it touched, and write-if-changed skips no-op rewrites.

use crate::plan::{Plan, Task};
use std::io;
use std::path::{Path, PathBuf};

/// Bumped only by an explicit one-off conversion; older code refuses a newer store.
pub const STORE_VERSION: u32 = 2;

/// The plan's yaml store lives beside the global plan database.
pub fn plan_state_dir(plan: &Plan) -> io::Result<PathBuf> {
    super::paths::plan_state_dir(
        super::paths::plan_sidecar_root(plan),
        &plan.status,
        &plan.id,
    )
}

fn task_stem(task: &Task) -> String {
    super::tag_model::task_handle(task).to_string()
}

pub fn tasks_dir(plan_dir: &Path) -> PathBuf {
    plan_dir.join("tasks")
}

pub fn task_yaml_path(plan_dir: &Path, task: &Task) -> PathBuf {
    tasks_dir(plan_dir).join(format!("{}.yaml", task_stem(task)))
}

pub fn plan_yaml_path(plan_dir: &Path) -> PathBuf {
    plan_dir.join("plan.yaml")
}

pub fn goal_yaml_path(plan_dir: &Path) -> PathBuf {
    plan_dir.join("goal.yaml")
}

/// The plan-own view: everything except the entities that own their own files.
/// Tasks and goal are stripped; sections stay inline (they are plan-own).
fn plan_meta(plan: &Plan) -> Plan {
    let mut meta = plan.clone();
    meta.tasks.clear();
    meta.goal = None;
    meta
}

fn to_yaml<T: serde::Serialize>(value: &T) -> io::Result<String> {
    serde_yaml::to_string(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn from_yaml<T: serde::de::DeserializeOwned>(text: &str) -> io::Result<T> {
    serde_yaml::from_str(text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Write only if the on-disk content differs — the property that keeps
/// untouched files out of git and off the disk.
fn write_if_changed(path: &Path, content: &str) -> io::Result<bool> {
    if matches!(std::fs::read_to_string(path), Ok(existing) if existing == content) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    super::atomic_write::write_atomic(path, content)?;
    Ok(true)
}

/// Persist a single task — the per-entity write. Touches exactly one file.
pub fn write_task(plan_dir: &Path, task: &Task) -> io::Result<bool> {
    write_if_changed(&task_yaml_path(plan_dir, task), &to_yaml(task)?)
}

/// Persist the whole plan as separate files. Still one call, but each file
/// is write-if-changed, so a save after a single-field mutation rewrites
/// only the affected file(s).
pub fn write_plan(plan_dir: &Path, plan: &Plan) -> io::Result<()> {
    std::fs::create_dir_all(tasks_dir(plan_dir))?;
    write_if_changed(&plan_yaml_path(plan_dir), &to_yaml(&plan_meta(plan))?)?;
    if let Some(goal) = &plan.goal {
        write_if_changed(&goal_yaml_path(plan_dir), &to_yaml(goal)?)?;
    }
    let mut keep: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    for task in &plan.tasks {
        let path = task_yaml_path(plan_dir, task);
        write_if_changed(&path, &to_yaml(task)?)?;
        keep.insert(path);
    }
    prune_stale_tasks(plan_dir, &keep)?;
    Ok(())
}

/// Remove task yaml files no longer backed by a live task (renamed handle,
/// deleted task). Never touches the sibling .jsonl (history is preserved).
fn prune_stale_tasks(
    plan_dir: &Path,
    keep: &std::collections::BTreeSet<PathBuf>,
) -> io::Result<()> {
    let dir = tasks_dir(plan_dir);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("yaml") && !keep.contains(&path) {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Load a plan by walking its directory. Never writes. A corrupt single
/// task is skipped with a loud log — the plan survives (the lesson from the
/// torn-strides incident that vanished whole plans).
pub fn load_plan(plan_dir: &Path) -> io::Result<Plan> {
    let meta_text = std::fs::read_to_string(plan_yaml_path(plan_dir))?;
    let mut plan: Plan = from_yaml(&meta_text)?;

    if let Ok(goal_text) = std::fs::read_to_string(goal_yaml_path(plan_dir)) {
        plan.goal = Some(from_yaml(&goal_text)?);
    }

    plan.tasks.clear();
    let dir = tasks_dir(plan_dir);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut task_files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("yaml"))
            .collect();
        task_files.sort();
        for path in task_files {
            match std::fs::read_to_string(&path).and_then(|t| from_yaml::<Task>(&t)) {
                Ok(task) => plan.tasks.push(task),
                Err(e) => log::error!(
                    "[plan.store_v2] skipping corrupt task file {}: {}",
                    path.display(),
                    e
                ),
            }
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests;
