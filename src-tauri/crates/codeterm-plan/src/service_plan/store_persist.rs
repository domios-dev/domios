use super::*;

impl PlanStore {
    #[cfg(test)]
    pub(crate) fn set_persistence_hook_for_test(
        &self,
        hook: Option<Arc<dyn Fn(&Plan) + Send + Sync>>,
    ) {
        *self.persistence_hook.write() = hook;
    }

    /// Persist one canonical plan transaction, including its owned spec.
    pub(super) fn persist_plan_and_spec_in_writer(
        &self,
        turn: &MutationWriterTurn<'_>,
        plan: &Plan,
    ) -> std::io::Result<PathBuf> {
        turn.assert_store(self);
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
        let writes = self
            .database
            .persist_plan_rows(plan, owned.as_ref())
            .map_err(plan_io_error)?;
        let path = self.database.path();
        #[cfg(test)]
        self.persist_test_sidecars(plan, owned.as_ref())?;
        if let Some((project_path, canonical_project_path)) = writes.previous_project_paths.clone()
        {
            let mut moved_from = Plan::new(
                String::new(),
                project_path,
                crate::plan::OrchestratorRef::default(),
            );
            moved_from.id = plan.id.clone();
            moved_from.canonical_project_path = canonical_project_path;
            super::search_index::enqueue_plan_removal(turn, moved_from);
        }
        super::search_index::enqueue_plan_index(turn, plan.clone(), index_scope(writes));
        if let Some(spec) = owned {
            super::search_index::enqueue_spec_index(turn, spec.clone());
            #[cfg(test)]
            let spec_path = self.test_spec_path(&spec).unwrap_or_else(|| path.clone());
            #[cfg(not(test))]
            let spec_path = path.clone();
            let entry = Self::spec_index_entry(&spec, spec_path);
            self.spec_index.write().insert(spec.id.clone(), entry);
        }
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

    pub(super) fn persist_spec_in_writer(
        &self,
        turn: &MutationWriterTurn<'_>,
        spec: &Spec,
    ) -> std::io::Result<PathBuf> {
        turn.assert_store(self);
        let path = self.database.persist_spec(spec).map_err(plan_io_error)?;
        super::search_index::enqueue_spec_index(turn, spec.clone());
        #[cfg(test)]
        if let Some(test_path) = self.persist_test_spec(spec)? {
            return Ok(test_path);
        }
        Ok(path)
    }

    #[cfg(test)]
    fn persist_test_sidecars(&self, plan: &Plan, owned: Option<&Spec>) -> std::io::Result<()> {
        let sidecar_root = paths::plan_sidecar_root(plan);
        if paths::test_project_root(sidecar_root).is_none() {
            return Ok(());
        }
        let root = paths::project_root(sidecar_root);
        let mut normalized = plan.clone();
        paths::normalize_plan_paths(&mut normalized, &root);
        tag_model::normalize_plan(&mut normalized);
        let dir = store_v2::plan_state_dir(&normalized)?;
        store_v2::write_plan(&dir, &normalized)?;
        Self::remove_other_status_dir(&normalized, &store_v2::plan_yaml_path(&dir))?;
        if let Some(spec) = owned {
            let target = spec_store::spec_yaml_path_in_plan(&dir);
            self.persist_test_spec_at(spec, target, &root)?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn persist_test_spec(&self, spec: &Spec) -> std::io::Result<Option<PathBuf>> {
        let sidecar_root = paths::spec_sidecar_root(spec);
        if paths::test_project_root(sidecar_root).is_none() {
            return Ok(None);
        }
        let root = paths::project_root(sidecar_root);
        let path = if spec.plan_id.is_some() {
            self.test_spec_path(spec).unwrap_or_else(|| {
                spec_store::spec_yaml_path(&paths::specs_dir(&root).unwrap(), &spec.id)
            })
        } else {
            spec_store::spec_yaml_path(&paths::specs_dir(&root)?, &spec.id)
        };
        self.persist_test_spec_at(spec, path.clone(), &root)?;
        Ok(Some(path))
    }

    #[cfg(test)]
    fn persist_test_spec_at(&self, spec: &Spec, path: PathBuf, root: &Path) -> std::io::Result<()> {
        let mut normalized = spec.clone();
        paths::normalize_spec_paths(&mut normalized, root);
        tag_model::normalize_sections_for_spec(&mut normalized.sections);
        if normalized.plan_id.is_some() {
            let global = spec_store::spec_yaml_path(&paths::specs_dir(root)?, &normalized.id);
            if global != path {
                let _ = std::fs::remove_file(global);
            }
        }
        spec_store::write_spec_at(&path, &normalized)?;
        Ok(())
    }

    #[cfg(test)]
    fn test_spec_path(&self, spec: &Spec) -> Option<PathBuf> {
        if let Some(plan_id) = spec.plan_id.as_deref() {
            let plan = self.get(plan_id)?;
            let sidecar_root = paths::plan_sidecar_root(&plan);
            paths::test_project_root(sidecar_root)?;
            let root = paths::project_root(sidecar_root);
            let mut normalized = plan.clone();
            paths::normalize_plan_paths(&mut normalized, &root);
            let dir = store_v2::plan_state_dir(&normalized).ok()?;
            return Some(spec_store::spec_yaml_path_in_plan(&dir));
        }
        let sidecar_root = paths::spec_sidecar_root(spec);
        paths::test_project_root(sidecar_root)?;
        let root = paths::project_root(sidecar_root);
        Some(spec_store::spec_yaml_path(
            &paths::specs_dir(&root).ok()?,
            &spec.id,
        ))
    }
}

pub(super) fn index_scope(
    writes: super::store_sync::RowWrites,
) -> super::search_index::PlanIndexScope {
    log::debug!(
        "[plan.store] persisted rows: tasks={} milestones={} attempts={} sections={} elements={} strides={} heartbeat={} deleted={}",
        writes.tasks,
        writes.milestones,
        writes.attempts,
        writes.sections,
        writes.elements,
        writes.strides,
        writes.heartbeat,
        writes.deleted
    );
    if writes.header_identity_changed {
        super::search_index::PlanIndexScope::Full
    } else {
        super::search_index::PlanIndexScope::Tasks(writes.changed_tasks.into_iter().collect())
    }
}

fn plan_io_error(error: PlanError) -> std::io::Error {
    std::io::Error::other(error.to_string())
}
