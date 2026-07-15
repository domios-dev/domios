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

/// The plan's yaml store lives IN the project, tracked by git normally:
/// `<project>/.codeterm/plans/<bucket>/<plan_id>/`. This is the existing
/// sidecar location — v2 changes the FORMAT inside it (one yaml per entity,
/// no plan.json, no frontmatter), not the location. The search cache already
/// lives outside the repo in ~/.codeterm/plan-indexes and is unaffected.
pub fn plan_state_dir(plan: &Plan) -> io::Result<PathBuf> {
    let project = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.as_str()
    } else {
        plan.canonical_project_path.as_str()
    };
    let root = super::paths::project_root(Path::new(project));
    super::paths::plan_dir(&root, &plan.status, &plan.id)
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
mod tests {
    use super::*;
    use crate::plan::{Goal, OrchestratorRef, PlanMode, TaskStatus};
    use crate::service_plan::{NewTask, PlanStore};

    // Build a real, fully-populated Plan+Task via the production store, then
    // hand it to store_v2 — so the round-trip exercises the actual model, not
    // a hand-rolled subset of fields.
    fn sample_plan(dir: &Path) -> Plan {
        let store = PlanStore::new_for_test(dir);
        let mut plan = Plan::new(
            "Storage v2 roundtrip".into(),
            dir.display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_test".into();
        plan.mode = PlanMode::Execution;
        plan.goal = Some(Goal {
            vector: "prove the store round-trips".into(),
            vision: "multi\nline\nvision".into(),
            constraints: vec!["c1".into()],
            postulates: vec![],
        });
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "First task".into(),
                    handle: Some("first-task".into()),
                    body: "A body with\nmultiple lines\nas a block scalar.".into(),
                    executor_role: Some("backend-dev".into()),
                    role_guidance: Some("You are a careful Rust worker.".into()),
                    model: Some("claude/opus".into()),
                    model_rationale: Some("ordering risk".into()),
                    acceptance_criteria: vec!["compiles".into(), "round-trips".into()],
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        // A pane must exist before in_progress (write-time invariant); set it,
        // then flip status — this also exercises the lane fields in the yaml.
        store
            .fill_task_lane_if_empty(
                &plan.id,
                &task_id,
                Some("pane-x"),
                Some("lane/first"),
                None,
                3,
            )
            .unwrap();
        store
            .update_task(
                &plan.id,
                &task_id,
                None,
                None,
                None,
                None,
                Some("in_progress"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                4,
            )
            .unwrap();
        store.get(&plan.id).unwrap()
    }

    #[test]
    fn plan_round_trips_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let plan = sample_plan(dir.path());
        write_plan(dir.path(), &plan).unwrap();
        let loaded = load_plan(dir.path()).unwrap();
        assert_eq!(loaded.id, plan.id);
        assert_eq!(loaded.title, plan.title);
        assert_eq!(loaded.tasks.len(), 1);
        let t = &loaded.tasks[0];
        assert_eq!(t.body, plan.tasks[0].body);
        assert_eq!(t.executor_role.as_deref(), Some("backend-dev"));
        assert_eq!(
            t.role_guidance.as_deref(),
            Some("You are a careful Rust worker.")
        );
        assert_eq!(t.model.as_deref(), Some("claude/opus"));
        assert_eq!(t.status, TaskStatus::InProgress);
        assert_eq!(loaded.goal.as_ref().unwrap().vision, "multi\nline\nvision");
    }

    #[test]
    fn plan_yaml_roundtrip_team_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut plan = sample_plan(dir.path());
        plan.team_id = Some("team_abcd1234".into());
        write_plan(dir.path(), &plan).unwrap();
        let meta = std::fs::read_to_string(plan_yaml_path(dir.path())).unwrap();
        assert!(
            meta.contains("team_abcd1234"),
            "plan.yaml must persist team_id"
        );
        let loaded = load_plan(dir.path()).unwrap();
        assert_eq!(loaded.team_id.as_deref(), Some("team_abcd1234"));
    }

    #[test]
    fn plan_yaml_omits_team_id_when_teamless() {
        let dir = tempfile::tempdir().unwrap();
        let plan = sample_plan(dir.path());
        assert_eq!(plan.team_id, None);
        write_plan(dir.path(), &plan).unwrap();
        let loaded = load_plan(dir.path()).unwrap();
        assert_eq!(loaded.team_id, None);
    }

    #[test]
    fn plan_yaml_holds_no_task_copies() {
        let dir = tempfile::tempdir().unwrap();
        write_plan(dir.path(), &sample_plan(dir.path())).unwrap();
        let meta = std::fs::read_to_string(plan_yaml_path(dir.path())).unwrap();
        assert!(
            !meta.contains("First task"),
            "plan.yaml must not copy task data"
        );
        assert!(
            !meta.contains("block scalar"),
            "plan.yaml must not copy task bodies"
        );
    }

    #[test]
    fn single_task_write_touches_only_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let plan = sample_plan(dir.path());
        write_plan(dir.path(), &plan).unwrap();
        let plan_mtime = std::fs::metadata(plan_yaml_path(dir.path()))
            .unwrap()
            .modified()
            .unwrap();
        let mut t = plan.tasks[0].clone();
        t.status = TaskStatus::Done;
        assert!(
            write_task(dir.path(), &t).unwrap(),
            "changed task must write"
        );
        let plan_mtime_after = std::fs::metadata(plan_yaml_path(dir.path()))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            plan_mtime, plan_mtime_after,
            "plan.yaml must not move on a task write"
        );
    }

    #[test]
    fn identical_write_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let plan = sample_plan(dir.path());
        write_plan(dir.path(), &plan).unwrap();
        assert!(
            !write_task(dir.path(), &plan.tasks[0]).unwrap(),
            "no-op write must skip"
        );
    }

    #[test]
    fn renamed_handle_prunes_the_stale_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut plan = sample_plan(dir.path());
        write_plan(dir.path(), &plan).unwrap();
        assert!(task_yaml_path(dir.path(), &plan.tasks[0]).exists());
        let old = task_yaml_path(dir.path(), &plan.tasks[0]);
        plan.tasks[0].handle = Some("renamed-task".into());
        write_plan(dir.path(), &plan).unwrap();
        assert!(!old.exists(), "stale handle file must be pruned");
        assert!(task_yaml_path(dir.path(), &plan.tasks[0]).exists());
    }
}
