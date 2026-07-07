use super::*;

impl PlanStore {
    #[cfg(test)]
    fn file_path(plan: &Plan) -> std::io::Result<PathBuf> {
        paths::plan_json_path(paths::plan_sidecar_root(plan), &plan.status, &plan.id)
    }

    /// The single plan persist path: yaml store only (no plan.json, no md
    /// sidecar fan-out). Per-entity write-if-changed, so a mutation touches
    /// only the file(s) that changed. Returns the plan.yaml path for the index.
    pub(super) fn persist_plan(plan: &Plan) -> std::io::Result<PathBuf> {
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
    pub(super) fn persist_plan_and_spec(&self, plan: &Plan) -> std::io::Result<PathBuf> {
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
    pub(super) fn spec_yaml_target(&self, spec: &Spec) -> std::io::Result<PathBuf> {
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

    pub(super) fn persist_spec(&self, spec: &Spec) -> std::io::Result<PathBuf> {
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

    pub(super) fn note_plan_write(&self, plan: &Plan, path: &Path) {
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

    pub(super) fn note_spec_write(&self, _spec: &Spec, path: &Path) {
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
}
