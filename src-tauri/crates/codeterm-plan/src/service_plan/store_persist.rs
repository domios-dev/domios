use super::*;

impl PlanStore {
    #[cfg(test)]
    pub(crate) fn set_persistence_hook_for_test(
        &self,
        hook: Option<Arc<dyn Fn(&Plan) + Send + Sync>>,
    ) {
        *self.persistence_hook.write() = hook;
    }

    #[cfg(test)]
    fn file_path(plan: &Plan) -> std::io::Result<PathBuf> {
        paths::plan_json_path(paths::plan_sidecar_root(plan), &plan.status, &plan.id)
    }

    /// Persist one canonical plan transaction, including its owned spec.
    pub(super) fn persist_plan_and_spec(&self, plan: &Plan) -> std::io::Result<PathBuf> {
        #[cfg(test)]
        if let Some(hook) = self.persistence_hook.read().clone() {
            hook(plan);
        }
        let owned = {
            self.specs
                .read()
                .values()
                .find(|s| s.lock().plan_id.as_deref() == Some(plan.id.as_str()))
                .map(|spec| spec.lock().clone())
        };
        let path = self
            .database
            .persist_plan(plan, owned.as_ref())
            .map_err(plan_io_error)?;
        if let Some(spec) = owned {
            let entry = Self::spec_index_entry(&spec, path.clone());
            self.spec_index.write().insert(spec.id.clone(), entry);
        }
        Ok(path)
    }

    /// Legacy plan.json + md-sidecar writer. Retained ONLY for the one-off
    /// conversion tool and tests; the live daemon never calls this.
    #[cfg(test)]
    pub(super) fn write_atomic(plan: &Plan) -> std::io::Result<PathBuf> {
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

    #[cfg(test)]
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

    pub(super) fn persist_spec(&self, spec: &Spec) -> std::io::Result<PathBuf> {
        self.database.persist_spec(spec).map_err(plan_io_error)
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

fn plan_io_error(error: PlanError) -> std::io::Error {
    std::io::Error::other(error.to_string())
}
