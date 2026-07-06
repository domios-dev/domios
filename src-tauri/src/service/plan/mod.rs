pub mod atomic_write;
pub mod attachments;
pub mod batch;
pub mod body_files;
pub mod canvas_elements;
pub mod cleanup_debt;
pub mod competition;
pub mod complete_wrapup;
pub mod convert_storage;
pub mod digest_rules;
pub mod element_queries;
pub mod gates;
pub mod handoff;
pub mod heartbeat;
pub mod hints;
pub mod ingest;
mod lane_metadata;
pub mod lifecycle;
pub mod locate;
pub mod loop_mutations;
pub mod milestone_gates;
pub mod mutations;
pub mod next_actions;
pub mod paths;
pub mod peer_mandate;
pub mod plan_skills;
pub mod qa;
mod readback;
mod reconcile_fixes;
pub mod reflection;
pub mod relink;
pub mod reminders;
pub mod resolve;
pub mod retrospective;
pub mod search_index;
pub mod section_links;
pub mod section_queries;
pub mod skill_registry;
pub mod spec;
pub mod spec_render;
pub mod spec_store;
pub mod state;
pub mod store_v2;
pub mod stride_log;
pub mod tag_model;
pub mod task_attempts;
pub mod task_queries;
mod task_remove;
mod types;
pub mod usage_digest;
pub mod watcher;
pub mod worker_charter;

#[cfg(test)]
mod body_file_tests;
#[cfg(test)]
mod canonical_path_tests;
#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod goal_tests;
#[cfg(test)]
mod heartbeat_tests;
#[cfg(test)]
mod readback_tests;
#[cfg(test)]
mod reminder_tests;
#[cfg(test)]
mod subtask_close_tests;
#[cfg(test)]
mod task_attempt_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod timestamp_tests;
#[cfg(test)]
mod watch_reconcile_tests;

use crate::plan::{Plan, PlanMode, PlanStatus, Spec, TaskStatus};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

pub use types::{
    IndexEntry, NewTask, OpenSubtask, PlanError, ReconcileFixResult, SpecIndexEntry, TaskLaneUpdate,
};

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
        #[cfg(test)]
        paths::mark_test_project_root(_root);
        Self::default()
    }

    #[cfg(test)]
    fn file_path(plan: &Plan) -> std::io::Result<PathBuf> {
        paths::plan_json_path(paths::plan_sidecar_root(plan), &plan.status, &plan.id)
    }

    /// The single plan persist path: yaml store only (no plan.json, no md
    /// sidecar fan-out). Per-entity write-if-changed, so a mutation touches
    /// only the file(s) that changed. Returns the plan.yaml path for the index.
    fn persist_plan(plan: &Plan) -> std::io::Result<PathBuf> {
        let mut normalized = plan.clone();
        if normalized.canonical_project_path.trim().is_empty() {
            normalized.canonical_project_path = normalized.project_path.clone();
        }
        tag_model::normalize_plan(&mut normalized);
        let dir = store_v2::plan_state_dir(&normalized)?;
        store_v2::write_plan(&dir, &normalized)?;
        Self::remove_other_status_dir(plan, &store_v2::plan_yaml_path(&dir))?;
        Ok(store_v2::plan_yaml_path(&dir))
    }

    /// persist_plan + carry the co-located owned spec. A status move
    /// (active<->archive) relocates the plan dir and remove_other_status_dir
    /// wipes the old one INCLUDING spec.yaml — re-persist the spec into
    /// wherever the plan now lives (write-if-changed: no-op otherwise).
    fn persist_plan_and_spec(&self, plan: &Plan) -> std::io::Result<PathBuf> {
        let path = Self::persist_plan(plan)?;
        let owned = {
            self.specs
                .read()
                .values()
                .find(|s| s.lock().plan_id.as_deref() == Some(plan.id.as_str()))
                .cloned()
        };
        if let Some(spec) = owned {
            let guard = spec.lock();
            // Target from the fresh plan, not the index — mid-transition the
            // index still points at the old status dir.
            let dir = store_v2::plan_state_dir(plan)?;
            let target = spec_store::spec_yaml_path_in_plan(&dir);
            let spec_path = self.persist_spec_at(&guard, target)?;
            self.note_spec_write(&guard, &spec_path);
            let entry = Self::spec_index_entry(&guard, spec_path);
            self.spec_index.write().insert(guard.id.clone(), entry);
        }
        Ok(path)
    }

    /// Legacy plan.json + md-sidecar writer. Retained ONLY for the one-off
    /// conversion tool and tests; the live daemon never calls this.
    #[cfg(test)]
    fn write_atomic(plan: &Plan) -> std::io::Result<PathBuf> {
        let path = Self::file_path(plan)?;
        let mut normalized = plan.clone();
        if normalized.canonical_project_path.trim().is_empty() {
            normalized.canonical_project_path = normalized.project_path.clone();
        }
        tag_model::normalize_plan(&mut normalized);
        body_files::persist_bodies(&normalized)?;
        let json_plan = body_files::strip_bodies(&normalized);
        let json = serde_json::to_string_pretty(&json_plan)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        atomic_write::write_atomic(&path, json)?;
        Self::remove_other_status_dir(plan, &path)?;
        Ok(path)
    }

    fn remove_other_status_dir(plan: &Plan, written_path: &Path) -> std::io::Result<()> {
        let project = paths::plan_sidecar_root(plan);
        for status in [PlanStatus::Active, PlanStatus::Archived] {
            if paths::plan_bucket(&status) == paths::plan_bucket(&plan.status) {
                continue;
            }
            let dir = paths::plan_dir(project, &status, &plan.id)?;
            if !written_path.starts_with(&dir) && dir.is_dir() {
                std::fs::remove_dir_all(dir)?;
            }
        }
        Ok(())
    }

    /// Spec v2 write target: owned spec -> `<plan_dir>/spec.yaml`, orphan ->
    /// `.codeterm/specs/<id>.yaml`. Resolves the plan dir via the INDEX (map
    /// lock only) — never the plan mutex, so spec persists can run while a
    /// plan guard is held on this thread (no ABBA / reentrant deadlock).
    fn spec_yaml_target(&self, spec: &Spec) -> std::io::Result<PathBuf> {
        if let Some(pid) = spec.plan_id.as_deref() {
            let plan_dir = self
                .index
                .read()
                .get(pid)
                .and_then(|e| e.file_path.parent().map(Path::to_path_buf));
            if let Some(dir) = plan_dir {
                return Ok(spec_store::spec_yaml_path_in_plan(&dir));
            }
        }
        let specs_dir = paths::specs_dir(paths::spec_sidecar_root(spec))?;
        Ok(spec_store::spec_yaml_path(&specs_dir, &spec.id))
    }

    fn persist_spec(&self, spec: &Spec) -> std::io::Result<PathBuf> {
        let path = self.spec_yaml_target(spec)?;
        self.persist_spec_at(spec, path)
    }

    fn persist_spec_at(&self, spec: &Spec, path: PathBuf) -> std::io::Result<PathBuf> {
        let mut normalized = spec.clone();
        if normalized.canonical_project_path.trim().is_empty() {
            normalized.canonical_project_path = normalized.project_path.clone();
        }
        tag_model::normalize_sections_for_spec(&mut normalized.sections);
        // A spec owned by a plan lives at <plan>/spec.yaml; when it was
        // previously orphaned, drop the stale global yaml so there is one copy.
        if normalized.plan_id.is_some() {
            let specs_dir = paths::specs_dir(paths::spec_sidecar_root(&normalized))?;
            let global = spec_store::spec_yaml_path(&specs_dir, &normalized.id);
            if global != path {
                let _ = std::fs::remove_file(global);
            }
        }
        spec_store::write_spec_at(&path, &normalized)?;
        Ok(path)
    }

    fn note_write(&self, path: &Path) {
        // Canonicalize so the key is stable across \\?\ vs plain-drive representations on Windows.
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Ok(meta) = std::fs::metadata(&key) {
            if let Ok(modified) = meta.modified() {
                self.last_writes.write().insert(key, modified);
            }
        }
    }

    fn note_plan_write(&self, plan: &Plan, path: &Path) {
        self.note_write(path);
        // Track the v2 yaml files this plan writes, so the watcher recognizes
        // its own writes and does not reconcile-storm on them.
        if let Ok(dir) = store_v2::plan_state_dir(plan) {
            self.note_write(&store_v2::plan_yaml_path(&dir));
            self.note_write(&store_v2::goal_yaml_path(&dir));
            for task in &plan.tasks {
                self.note_write(&store_v2::task_yaml_path(&dir, task));
            }
        }
        // Legacy body paths too, harmless while both formats can exist on disk.
        if let Ok(body_paths) = body_files::body_paths(plan) {
            for path in body_paths {
                self.note_write(&path);
            }
        }
    }

    fn note_spec_write(&self, _spec: &Spec, path: &Path) {
        self.note_write(path);
    }

    pub fn is_own_recent_write(&self, path: &Path) -> bool {
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let Ok(meta) = std::fs::metadata(&key) else {
            return false;
        };
        let Ok(modified) = meta.modified() else {
            return false;
        };
        self.last_writes
            .read()
            .get(&key)
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
                .filter(|task| !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled))
                .filter_map(|t| {
                    let pane_id = t.assigned_pane_id.as_deref()?.trim();
                    (!pane_id.is_empty()).then(|| pane_id.to_string())
                })
                .collect(),
            task_count: plan.tasks.len(),
            open_task_count: plan
                .tasks
                .iter()
                .filter(|t| t.status.counts_as_open_work())
                .count(),
            section_count: plan.sections.len(),
            element_count: plan.elements.len(),
            loop_count: plan.loops.len(),
            has_goal: plan.goal.is_some(),
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
        let existing = { self.plans.read().get(&id).cloned() };
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
        let existing = { self.specs.read().get(&id).cloned() };
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
        let path = self
            .persist_plan_and_spec(&committed)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_plan_write(&committed, &path);
        if let Err(e) = self.mirror_plan_index(&committed) {
            log::warn!(
                "[plan.index] mirror lag in commit_loaded_plan for {}: {}",
                committed.id,
                e
            );
        }
        let entry = Self::index_entry(&committed, path);
        self.index.write().insert(committed.id.clone(), entry);
        self.reset_bound_panes(&committed);
        Ok(())
    }

    pub(super) fn commit_loaded_spec(&self, spec: Spec) -> Result<(), PlanError> {
        let committed = self.upsert_spec_arc(spec);
        let path = self
            .persist_spec(&committed)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_spec_write(&committed, &path);
        if let Err(e) = self.mirror_spec_index(&committed) {
            log::warn!(
                "[plan.index] mirror lag in commit_loaded_spec for {}: {}",
                committed.id,
                e
            );
        }
        let entry = Self::spec_index_entry(&committed, path);
        self.spec_index.write().insert(committed.id.clone(), entry);
        Ok(())
    }

    fn load_file_into_index(&self, path: PathBuf, project_root: &Path) -> Result<(), PlanError> {
        // v2 plan: load straight from the yaml hierarchy (no plan.json, no
        // hydrate-and-rewrite). Legacy plan: the json + body-file path below.
        let mut plan = if path.file_name().and_then(|n| n.to_str()) == Some("plan.yaml") {
            let dir = path
                .parent()
                .ok_or_else(|| PlanError::Io("plan.yaml has no parent dir".into()))?;
            store_v2::load_plan(dir).map_err(|e| PlanError::Io(e.to_string()))?
        } else {
            let json = std::fs::read_to_string(&path).map_err(|e| PlanError::Io(e.to_string()))?;
            let mut plan: Plan =
                serde_json::from_str(&json).map_err(|e| PlanError::Io(e.to_string()))?;
            let migrated = body_files::hydrate_and_migrate(&mut plan)
                .map_err(|e| PlanError::Io(e.to_string()))?;
            if migrated {
                let wp = Self::persist_plan(&plan).map_err(|e| PlanError::Io(e.to_string()))?;
                self.note_plan_write(&plan, &wp);
            }
            plan
        };
        paths::normalize_plan_paths(&mut plan, project_root);
        tag_model::normalize_plan(&mut plan);
        let committed = self.upsert_plan_arc(plan);
        self.note_plan_write(&committed, &path);
        let entry = Self::index_entry(&committed, path.clone());
        self.index.write().insert(committed.id.clone(), entry);
        // Co-located spec.yaml rides with its plan (startup scan + reconcile).
        if let Some(dir) = path.parent() {
            let spec_yaml = dir.join("spec.yaml");
            if spec_yaml.is_file() {
                if let Err(e) = self.load_spec_file_into_index(spec_yaml, project_root) {
                    log::warn!("[spec] skip malformed co-located spec: {e}");
                }
            }
        }
        Ok(())
    }

    fn load_spec_file_into_index(
        &self,
        path: PathBuf,
        project_root: &Path,
    ) -> Result<(), PlanError> {
        let mut spec: Spec =
            spec_store::load_spec_from_path(&path).map_err(|e| PlanError::Io(e.to_string()))?;
        paths::normalize_spec_paths(&mut spec, project_root);
        let committed = self.upsert_spec_arc(spec);
        self.note_spec_write(&committed, &path);
        let entry = Self::spec_index_entry(&committed, path);
        self.spec_index.write().insert(committed.id.clone(), entry);
        Ok(())
    }

    pub fn scan_project(&self, project_path: &Path) -> Result<PathBuf, PlanError> {
        let project_root = paths::project_root(project_path);
        let plans_dir =
            paths::plans_dir(&project_root).map_err(|e| PlanError::Io(e.to_string()))?;
        for bucket in ["active", "archive"] {
            let bucket_dir = plans_dir.join(bucket);
            let Ok(entries) = std::fs::read_dir(&bucket_dir) else {
                continue;
            };
            for entry in entries {
                let Ok(entry) = entry else {
                    continue;
                };
                // v2-first discovery: a converted plan dir has plan.yaml and no
                // plan.json; a legacy dir has plan.json. Find either.
                let dir = entry.path();
                let yaml = dir.join("plan.yaml");
                let json = dir.join("plan.json");
                let path = if yaml.is_file() {
                    yaml
                } else if json.is_file() {
                    json
                } else {
                    continue;
                };
                if let Err(e) = self.load_file_into_index(path.clone(), &project_root) {
                    log::warn!("[plan] skip malformed plan {}: {}", path.display(), e);
                }
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
                if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
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
        if plan.canonical_project_path.trim().is_empty() {
            plan.canonical_project_path = plan.project_path.clone();
        }
        plan.updated_at = now_ms;
        tag_model::normalize_plan(plan);
        let id = plan.id.clone();
        let existing = { self.plans.read().get(&id).cloned() };
        let arc = if let Some(arc) = existing {
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
        let path = self.persist_plan_and_spec(&guard)?;
        self.note_plan_write(&guard, &path);
        if let Err(e) = self.mirror_plan_index(&guard) {
            log::warn!("[plan.index] mirror lag in save for {}: {}", guard.id, e);
        }
        self.index
            .write()
            .insert(guard.id.clone(), Self::index_entry(&guard, path));
        Ok(())
    }

    pub fn save_spec(&self, spec: &mut Spec, now_ms: u64) -> std::io::Result<()> {
        if spec.created_at == 0 {
            spec.created_at = now_ms;
        }
        if spec.canonical_project_path.trim().is_empty() {
            spec.canonical_project_path = spec.project_path.clone();
        }
        spec.updated_at = now_ms;
        tag_model::normalize_sections_for_spec(&mut spec.sections);
        let id = spec.id.clone();
        let existing = { self.specs.read().get(&id).cloned() };
        let arc = if let Some(arc) = existing {
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
        let path = self.persist_spec(&guard)?;
        self.note_spec_write(&guard, &path);
        if let Err(e) = self.mirror_spec_index(&guard) {
            log::warn!(
                "[plan.index] mirror lag in save_spec for {}: {}",
                guard.id,
                e
            );
        }
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

    pub fn spec_document_markdown(&self, id: &str) -> Option<String> {
        self.get_spec(id)
            .map(|spec| spec_render::spec_markdown(&spec))
    }

    pub fn reload_from_disk(&self, id: &str) -> Result<(), PlanError> {
        let path = self
            .index
            .read()
            .get(id)
            .map(|e| e.file_path.clone())
            .ok_or(PlanError::NotFound)?;
        // v2-first: a converted plan has plan.yaml — load it directly (walk the
        // hierarchy, no plan.json, no hydrate-and-rewrite churn). Fall through
        // to the legacy json path for plans not yet converted.
        if let Some(dir) = path.parent() {
            if store_v2::plan_yaml_path(dir).exists() {
                let mut plan =
                    store_v2::load_plan(dir).map_err(|e| PlanError::Io(e.to_string()))?;
                if plan.id != id {
                    return Err(PlanError::Io(format!(
                        "plan id mismatch: expected {id}, found {}",
                        plan.id
                    )));
                }
                let project_root = paths::project_root(Path::new(&plan.project_path));
                paths::normalize_plan_paths(&mut plan, &project_root);
                tag_model::normalize_plan(&mut plan);
                let committed = self.upsert_plan_arc(plan);
                let _ = self.mirror_plan_index(&committed);
                self.index
                    .write()
                    .insert(committed.id.clone(), Self::index_entry(&committed, path));
                return Ok(());
            }
        }
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
        paths::normalize_plan_paths(&mut plan, &project_root);
        let migrated =
            body_files::hydrate_and_migrate(&mut plan).map_err(|e| PlanError::Io(e.to_string()))?;
        tag_model::normalize_plan(&mut plan);
        if migrated {
            let path = Self::persist_plan(&plan).map_err(|e| PlanError::Io(e.to_string()))?;
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
        if pane_id.trim().is_empty() {
            return None;
        }
        for plan in self.plans.read().values() {
            let plan = plan.lock();
            if plan.status != PlanStatus::Active {
                continue;
            }
            if let Some(task) = plan
                .tasks
                .iter()
                .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
                .find(|t| t.assigned_pane_id.as_deref() == Some(pane_id))
            {
                return Some((plan.id.clone(), task.id.clone()));
            }
        }
        None
    }

    pub fn report_pending_task_binding_for_pane(&self, pane_id: &str) -> Option<(String, String)> {
        if pane_id.trim().is_empty() {
            return None;
        }
        for plan in self.plans.read().values() {
            let plan = plan.lock();
            if plan.status != PlanStatus::Active || plan.mode != PlanMode::Execution {
                continue;
            }
            if let Some(task) = plan
                .tasks
                .iter()
                .filter(|t| {
                    matches!(
                        t.status,
                        TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::Blocked
                    )
                })
                .find(|t| t.assigned_pane_id.as_deref() == Some(pane_id))
            {
                return Some((plan.id.clone(), task.id.clone()));
            }
        }
        None
    }

    pub fn active_owned_plan_with_open_work_for_pane(&self, pane_id: &str) -> Option<String> {
        if pane_id.trim().is_empty() {
            return None;
        }
        for plan in self.plans.read().values() {
            let plan = plan.lock();
            if plan.status != PlanStatus::Active
                || plan.mode != PlanMode::Execution
                || plan.owner.pane_id.as_deref() != Some(pane_id)
            {
                continue;
            }
            if plan.tasks.iter().any(|t| t.status.counts_as_open_work()) {
                return Some(plan.id.clone());
            }
        }
        None
    }

    pub fn active_owned_plan_frontier_for_pane(
        &self,
        pane_id: &str,
    ) -> Option<(String, usize, usize)> {
        if pane_id.trim().is_empty() {
            return None;
        }
        for plan in self.plans.read().values() {
            let plan = plan.lock();
            if plan.status != PlanStatus::Active
                || plan.mode != PlanMode::Execution
                || plan.owner.pane_id.as_deref() != Some(pane_id)
            {
                continue;
            }
            let pending = plan
                .tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Pending)
                .count();
            if pending == 0 {
                continue;
            }
            return Some((plan.id.clone(), pending, plan.heartbeat.active_lanes.len()));
        }
        None
    }

    pub fn task_bindings_for_pane(&self, pane_id: &str) -> Vec<(String, String)> {
        if pane_id.trim().is_empty() {
            return Vec::new();
        }
        self.list_full()
            .into_iter()
            .filter(|plan| plan.status == PlanStatus::Active)
            .flat_map(|plan| {
                let plan_id = plan.id.clone();
                plan.tasks
                    .into_iter()
                    .filter(|task| !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled))
                    .filter(move |task| task.assigned_pane_id.as_deref() == Some(pane_id))
                    .map(move |task| (plan_id.clone(), task.id))
            })
            .collect()
    }

    /// Like `task_bindings_for_pane` but also returns the task name for error messages.
    pub fn task_bindings_with_name_for_pane(&self, pane_id: &str) -> Vec<(String, String, String)> {
        if pane_id.trim().is_empty() {
            return Vec::new();
        }
        self.list_full()
            .into_iter()
            .filter(|plan| plan.status == PlanStatus::Active)
            .flat_map(|plan| {
                let plan_id = plan.id.clone();
                plan.tasks
                    .into_iter()
                    .filter(|task| !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled))
                    .filter(move |task| task.assigned_pane_id.as_deref() == Some(pane_id))
                    .map(move |task| (plan_id.clone(), task.id, task.name))
            })
            .collect()
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
        let (plan, entry_path) = self.forget_plan(id);
        if let Some(path) = entry_path {
            let _ = std::fs::remove_file(&path);
        }
        if let Some(plan) = plan {
            let _ = body_files::remove_body_dir(&plan);
        }
        Ok(())
    }

    /// Drop a plan from the in-memory index + search units without touching disk.
    /// The plan's owned spec (1:1, co-located) lives and dies with it.
    fn forget_plan(&self, id: &str) -> (Option<Plan>, Option<PathBuf>) {
        let plan = self
            .plans
            .write()
            .remove(id)
            .map(|plan| plan.lock().clone());
        let entry_path = self.index.write().remove(id).map(|e| e.file_path);
        if let Some(plan) = plan.as_ref() {
            let _ = search_index::delete_plan_units(plan);
        }
        self.forget_owned_spec(id);
        (plan, entry_path)
    }

    fn forget_owned_spec(&self, plan_id: &str) {
        let owned: Vec<String> = self
            .specs
            .read()
            .iter()
            .filter(|(_, spec)| spec.lock().plan_id.as_deref() == Some(plan_id))
            .map(|(id, _)| id.clone())
            .collect();
        for spec_id in owned {
            self.specs.write().remove(&spec_id);
            self.spec_index.write().remove(&spec_id);
        }
    }

    fn find_plan_file_on_disk(&self, project_root: &Path, plan_id: &str) -> Option<PathBuf> {
        for status in [PlanStatus::Active, PlanStatus::Archived] {
            let json = paths::plan_json_path_unchecked(project_root, &status, plan_id);
            let yaml = store_v2::plan_yaml_path(json.parent()?);
            if yaml.is_file() {
                return Some(yaml);
            }
            if json.is_file() {
                return Some(json);
            }
        }
        None
    }

    /// True when any file of the plan's on-disk state (plan.yaml, goal.yaml,
    /// spec.yaml, tasks/*.yaml) was modified by someone other than us.
    fn plan_dir_externally_dirty(&self, plan_yaml: &Path) -> bool {
        if !self.is_own_recent_write(plan_yaml) {
            return true;
        }
        let Some(dir) = plan_yaml.parent() else {
            return false;
        };
        for name in ["goal.yaml", "spec.yaml"] {
            let p = dir.join(name);
            if p.is_file() && !self.is_own_recent_write(&p) {
                return true;
            }
        }
        if let Ok(entries) = std::fs::read_dir(dir.join("tasks")) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("yaml")
                    && !self.is_own_recent_write(&p)
                {
                    return true;
                }
            }
        }
        false
    }

    /// Reconcile one plan against its on-disk sidecar after an external FS event.
    /// Loaded=added/updated, Removed=external delete, Unchanged=own write, Absent=nothing.
    pub fn reconcile_watched_plan(
        &self,
        project_root: &Path,
        plan_id: &str,
    ) -> Result<WatchOutcome, PlanError> {
        match self.find_plan_file_on_disk(project_root, plan_id) {
            Some(path) => {
                // Own-write suppression must consider every per-entity file: a
                // hand-edit of tasks/<h>.yaml or spec.yaml arrives while
                // plan.yaml itself is still our own untouched write.
                if !self.plan_dir_externally_dirty(&path) {
                    return Ok(WatchOutcome::Unchanged);
                }
                self.load_file_into_index(path, project_root)?;
                Ok(WatchOutcome::Loaded)
            }
            None => {
                let known = self.plans.read().contains_key(plan_id)
                    || self.index.read().contains_key(plan_id);
                if known {
                    self.forget_plan(plan_id);
                    Ok(WatchOutcome::Removed)
                } else {
                    Ok(WatchOutcome::Absent)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchOutcome {
    Loaded,
    Removed,
    Unchanged,
    Absent,
}

impl WatchOutcome {
    pub fn should_emit(self) -> bool {
        matches!(self, WatchOutcome::Loaded | WatchOutcome::Removed)
    }
}
