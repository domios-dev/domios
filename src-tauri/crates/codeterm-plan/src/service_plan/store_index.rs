use super::*;
use crate::plan::OrchestratorRef;

fn append_lifecycle_event(plan: &mut Plan, kind: &str, at: u64, author: OrchestratorRef) {
    let id = format!("lifecycle-{kind}-{}", plan.lifecycle_events.len());
    plan.lifecycle_events.push(PlanLifecycleEvent {
        id,
        kind: kind.into(),
        at,
        author,
    });
}

pub(super) fn record_lifecycle_events(plan: &mut Plan, previous: Option<&Plan>, at: u64) {
    match previous {
        None => {
            append_lifecycle_event(plan, "opened", at, plan.owner.clone());
            match &plan.status {
                PlanStatus::Active => {
                    append_lifecycle_event(plan, "activated", at, plan.owner.clone());
                }
                PlanStatus::Archived => {
                    append_lifecycle_event(plan, "archived", at, plan.owner.clone());
                }
            }
        }
        Some(previous) if previous.status != plan.status => {
            match (&previous.status, &plan.status) {
                (PlanStatus::Active, PlanStatus::Archived) => {
                    append_lifecycle_event(plan, "archived", at, previous.owner.clone());
                }
                (PlanStatus::Archived, PlanStatus::Active) => {
                    append_lifecycle_event(plan, "activated", at, plan.owner.clone());
                }
                _ => {}
            }
        }
        Some(_) => {}
    }
}

impl PlanStore {
    /// The index keeps these six fields of every plan; the committed values replace the old row.
    pub(super) fn note_index(&self, plan: &Plan) {
        self.index.write().insert(
            plan.id.clone(),
            store_db::PlanIndexRow {
                id: plan.id.clone(),
                status: plan.status.clone(),
                team_id: plan.team_id.clone(),
                project_path: plan.project_path.clone(),
                title: plan.title.clone(),
                updated_at: plan.updated_at,
            },
        );
    }

    pub(super) fn spec_index_entry(spec: &Spec, file_path: PathBuf) -> SpecIndexEntry {
        SpecIndexEntry {
            spec_id: spec.id.clone(),
            file_path,
            title: spec.title.clone(),
            status: spec.status.clone(),
            updated_at: spec.updated_at,
            project_path: spec.project_path.clone(),
        }
    }

    /// Merges an incoming plan with the stored one, the newer `updated_at` winning. The
    /// returned handle holds the winner; callers cache it only after the write commits.
    fn upsert_plan_arc(&self, plan: Plan) -> (Plan, plan_cache::PlanHandle) {
        let Some(arc) = self.plan_cache.get(&plan.id) else {
            let stored_is_newer = self
                .index
                .read()
                .get(&plan.id)
                .is_some_and(|row| row.updated_at > plan.updated_at);
            let winner = if stored_is_newer {
                self.try_get(&plan.id).unwrap_or(plan)
            } else {
                plan
            };
            return (winner.clone(), Arc::new(Mutex::new(winner)));
        };
        let Some(mut guard) = arc.try_lock() else {
            let mut guard = arc.lock();
            if guard.updated_at < plan.updated_at {
                *guard = plan;
            }
            let committed = guard.clone();
            drop(guard);
            return (committed, arc);
        };
        if guard.updated_at <= plan.updated_at {
            *guard = plan;
        }
        let committed = guard.clone();
        drop(guard);
        (committed, arc)
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
        let (committed, handle) = self.upsert_plan_arc(plan);
        if let Err(error) = self.persist_plan_and_spec(&committed) {
            self.plan_cache.evict(&committed.id);
            return Err(PlanError::Io(error.to_string()));
        }
        self.plan_cache.commit(&committed.id, &handle);
        self.note_index(&committed);
        self.reset_bound_tabs(&committed);
        Ok(())
    }

    pub(super) fn commit_loaded_spec(&self, spec: Spec) -> Result<(), PlanError> {
        let committed = self.upsert_spec_arc(spec);
        let path = self
            .persist_spec(&committed)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        let entry = Self::spec_index_entry(&committed, path);
        self.spec_index.write().insert(committed.id.clone(), entry);
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn load_file_into_index(
        &self,
        path: PathBuf,
        project_root: &Path,
    ) -> Result<(), PlanError> {
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
                self.persist_plan_and_spec(&plan)
                    .map_err(|e| PlanError::Io(e.to_string()))?;
            }
            plan
        };
        paths::normalize_plan_paths(&mut plan, project_root);
        tag_model::normalize_plan(&mut plan);
        // Load the disk spec before persisting so a hand-edited file is not overwritten by stale in-memory data.
        let colocated_spec = path.parent().and_then(|dir| {
            let spec_yaml = dir.join("spec.yaml");
            spec_yaml.is_file().then_some(spec_yaml)
        });
        if let Some(spec_yaml) = &colocated_spec {
            match spec_store::load_spec_from_path(spec_yaml) {
                Ok(mut spec) => {
                    paths::normalize_spec_paths(&mut spec, project_root);
                    self.upsert_spec_arc(spec);
                }
                Err(error) => log::warn!(
                    "[plan] co-located spec {} failed to load: {}",
                    spec_yaml.display(),
                    error
                ),
            }
        }
        // Persist the plan before its spec because the foreign key requires the owning plan row first.
        self.commit_loaded_plan(plan)?;
        // A malformed co-located spec must not take its plan down with it.
        if let Some(spec_yaml) = colocated_spec {
            if let Err(error) = self.load_spec_file_into_index(spec_yaml.clone(), project_root) {
                log::warn!(
                    "[plan] plan loaded but co-located spec {} skipped: {}",
                    spec_yaml.display(),
                    error
                );
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn load_spec_file_into_index(
        &self,
        path: PathBuf,
        project_root: &Path,
    ) -> Result<(), PlanError> {
        let mut spec: Spec =
            spec_store::load_spec_from_path(&path).map_err(|e| PlanError::Io(e.to_string()))?;
        paths::normalize_spec_paths(&mut spec, project_root);
        self.commit_loaded_spec(spec)
    }

    pub fn scan_project(&self, project_path: &Path) -> Result<PathBuf, PlanError> {
        Ok(paths::project_root(project_path))
    }

    pub fn relocate_workspace_projects(
        &self,
        tab_ids: &[String],
        team_ids: &[String],
        rebase: impl Fn(&str) -> Option<String>,
    ) -> std::io::Result<(Vec<String>, Vec<String>)> {
        let _writer_turn = self.mutation_writer.enter();
        let mut updated = Vec::new();
        let mut warnings = Vec::new();
        let candidates = PlanMatch::Any(vec![
            PlanMatch::Owners(tab_ids.to_vec()),
            PlanMatch::Teams(team_ids.to_vec()),
        ]);
        for mut plan in self.plans_matching(&candidates) {
            let bound = plan
                .owner
                .tab_id
                .as_ref()
                .is_some_and(|id| tab_ids.contains(id))
                || plan
                    .team_id
                    .as_ref()
                    .is_some_and(|id| team_ids.contains(id));
            if !bound {
                continue;
            }
            let mut changed = false;
            for (label, path) in [
                ("project", &mut plan.project_path),
                ("canonical project", &mut plan.canonical_project_path),
            ] {
                if path.is_empty() {
                    continue;
                }
                if let Some(new) = rebase(path) {
                    changed |= *path != new;
                    *path = new;
                } else {
                    warnings.push(format!(
                        "Plan {} {label}: {path} is outside the old workspace folder; unchanged",
                        plan.id
                    ));
                }
            }
            if changed {
                self.save_in_writer(&mut plan, unix_time_millis())?;
                updated.push(plan.id);
            }
        }
        Ok((updated, warnings))
    }

    pub fn save(&self, plan: &mut Plan, now_ms: u64) -> std::io::Result<()> {
        let _writer_turn = self.mutation_writer.enter();
        self.save_in_writer(plan, now_ms)
    }

    pub(super) fn save_in_writer(&self, plan: &mut Plan, now_ms: u64) -> std::io::Result<()> {
        if plan.created_at == 0 {
            plan.created_at = now_ms;
        }
        if plan.canonical_project_path.trim().is_empty() {
            plan.canonical_project_path = plan.project_path.clone();
        }
        plan.updated_at = now_ms;
        tag_model::normalize_plan(plan);
        super::text_bounds::validate_plan(plan).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
        })?;
        let id = plan.id.clone();
        let existing = if self.index.read().contains_key(&id) {
            Some(
                self.plan_arc(&id)
                    .map_err(|error| std::io::Error::other(error.to_string()))?,
            )
        } else {
            None
        };
        let previous = existing.as_ref().map(|arc| arc.lock().clone());
        let mut staged = match &previous {
            Some(stored) if stored.updated_at > plan.updated_at => stored.clone(),
            _ => plan.clone(),
        };
        record_lifecycle_events(&mut staged, previous.as_ref(), now_ms);
        *plan = staged.clone();
        if let Err(error) = self.persist_plan_and_spec(&staged) {
            self.plan_cache.evict(&id);
            return Err(error);
        }
        let handle = match existing {
            Some(arc) => {
                *arc.lock() = staged.clone();
                arc
            }
            None => Arc::new(Mutex::new(staged.clone())),
        };
        self.plan_cache.commit(&id, &handle);
        self.note_index(&staged);
        Ok(())
    }

    pub fn save_spec(&self, spec: &mut Spec, now_ms: u64) -> std::io::Result<()> {
        let _writer_turn = self.mutation_writer.enter();
        if spec.created_at == 0 {
            spec.created_at = now_ms;
        }
        if spec.canonical_project_path.trim().is_empty() {
            spec.canonical_project_path = spec.project_path.clone();
        }
        spec.updated_at = now_ms;
        tag_model::normalize_sections_for_spec(&mut spec.sections);
        super::text_bounds::validate_spec(spec).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
        })?;
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
        let staged = {
            let guard = arc.lock();
            if guard.updated_at <= spec.updated_at {
                spec.clone()
            } else {
                guard.clone()
            }
        };
        *spec = staged.clone();
        let path = self.persist_spec(&staged)?;
        {
            let mut guard = arc.lock();
            *guard = staged.clone();
        }
        self.spec_index
            .write()
            .insert(staged.id.clone(), Self::spec_index_entry(&staged, path));
        Ok(())
    }

    pub fn reload_from_disk(&self, id: &str) -> Result<(), PlanError> {
        #[cfg(test)]
        let sidecar_reload_blocked = self
            .plan_cache
            .get(id)
            .is_some_and(|plan| plan.try_lock().is_none());
        #[cfg(test)]
        if !sidecar_reload_blocked {
            if let Some(plan) = self.get(id) {
                if let Some(root) = paths::test_project_root(paths::plan_sidecar_root(&plan)) {
                    let mut normalized = plan.clone();
                    paths::normalize_plan_paths(&mut normalized, &root);
                    let dir = store_v2::plan_state_dir(&normalized)
                        .map_err(|error| PlanError::Io(error.to_string()))?;
                    let yaml = store_v2::plan_yaml_path(&dir);
                    if yaml.is_file() {
                        return self.load_file_into_index(yaml, &root);
                    }
                }
            }
        }
        // The stored row is the truth: drop the cached copy so the next read loads it.
        let row = self
            .database
            .load_index_row(id)?
            .ok_or(PlanError::NotFound)?;
        self.plan_cache.evict(id);
        // A mutation that committed since the row was read is newer; its index values stand.
        let mut index = self.index.write();
        if index
            .get(id)
            .is_none_or(|current| current.updated_at <= row.updated_at)
        {
            index.insert(row.id.clone(), row);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_board_state_lifecycle_events_are_stored() {
        let root = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(root.path());
        let mut plan = Plan::new(
            "Team board".into(),
            root.path().display().to_string(),
            OrchestratorRef::from_tab("owner-tab"),
        );
        plan.team_id = Some("team-a".into());
        store.save(&mut plan, 10).unwrap();
        store.archive(&plan.id, 20).unwrap();
        store.unarchive(&plan.id, 30).unwrap();

        let reopened = PlanStore::open(store.database_path_for_test()).unwrap();
        let events = reopened.get(&plan.id).unwrap().lifecycle_events;
        assert_eq!(
            events
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["opened", "activated", "archived", "activated"]
        );
        assert_eq!(
            events.iter().map(|event| event.at).collect::<Vec<_>>(),
            vec![10, 10, 20, 30]
        );
        assert!(events
            .iter()
            .all(|event| event.author.tab_id.as_deref() == Some("owner-tab")));
    }
}

fn unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
