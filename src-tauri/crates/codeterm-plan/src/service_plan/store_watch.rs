use super::*;

impl PlanStore {
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
                let known = self.plans.read().contains_key(plan_id)
                    || self.index.read().contains_key(plan_id);
                // Own-write suppression must consider every per-entity file: a hand-edit of tasks/<h>.yaml or spec.yaml arrives while plan.yaml itself is still our own untouched write.
                if known && !self.plan_dir_externally_dirty(&path) {
                    return Ok(WatchOutcome::Unchanged);
                }
                self.load_file_into_index(path, project_root)?;
                Ok(WatchOutcome::Loaded)
            }
            None => {
                let known = self.plans.read().contains_key(plan_id)
                    || self.index.read().contains_key(plan_id);
                if self.database.load_plan(plan_id)?.is_some() {
                    if known {
                        return Ok(WatchOutcome::Unchanged);
                    }
                    self.reload_from_disk(plan_id)?;
                    return Ok(WatchOutcome::Loaded);
                }
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
