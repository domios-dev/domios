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

    #[cfg(test)]
    pub(super) fn load_file_into_index(
        &self,
        path: PathBuf,
        project_root: &Path,
    ) -> Result<(), PlanError> {
        let turn = self.mutation_writer.enter();
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
            body_files::hydrate_and_migrate(&mut plan)
                .map_err(|error| PlanError::Io(error.to_string()))?;
            plan
        };
        paths::normalize_plan_paths(&mut plan, project_root);
        tag_model::normalize_plan(&mut plan);
        let owned = path
            .parent()
            .map(|dir| dir.join("spec.yaml"))
            .filter(|file| file.is_file())
            .and_then(|file| match spec_store::load_spec_from_path(&file) {
                Ok(mut spec) => {
                    paths::normalize_spec_paths(&mut spec, project_root);
                    Some(spec)
                }
                Err(error) => {
                    log::warn!(
                        "[plan] co-located spec {} failed to load: {error}",
                        file.display()
                    );
                    None
                }
            });
        let receipt = self.database.import_plan_rows(&mut plan, owned.as_ref())?;
        if let Some(receipt) = receipt {
            self.publish_hot_receipt(&turn, receipt);
            super::search_index::enqueue_plan_import(&turn, &plan);
            self.persist_test_sidecars(&plan, owned.as_ref())
                .map_err(|error| PlanError::Io(error.to_string()))?;
        }
        Ok(())
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
        let turn = self.mutation_writer.enter();
        let (rows, warnings) = self
            .database
            .relocate_plan_rows(tab_ids, team_ids, rebase, unix_time_millis())
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let mut updated = vec![];
        for (receipt, old_root) in rows {
            updated.push(receipt.plan_id.clone());
            if let Some(new_root) = &receipt.index_root {
                super::search_index::enqueue_plan_move(
                    &turn,
                    &receipt.plan_id,
                    old_root,
                    new_root.clone(),
                );
            }
            self.publish_hot_receipt(&turn, receipt);
        }
        Ok((updated, warnings))
    }

    pub fn import_plan(&self, plan: &mut Plan, now_ms: u64) -> std::io::Result<()> {
        let writer_turn = self.mutation_writer.enter();
        self.import_payload_in_writer(&writer_turn, plan, now_ms)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn save(&self, plan: &mut Plan, now: u64) -> std::io::Result<()> {
        self.import_plan(plan, now)
    }

    pub fn import_fixture_plans<T, R>(
        &self,
        plans: &[Plan],
        commit_desk: impl FnOnce() -> Result<T, String>,
        publish: impl FnOnce(T) -> Result<R, String>,
    ) -> Result<R, String> {
        let turn = self.mutation_writer.enter();
        let mut ids = std::collections::HashSet::new();
        let mut teams = std::collections::HashSet::new();
        {
            let index = self.index.read();
            for plan in plans {
                if plan.id.is_empty() || !ids.insert(&plan.id) || index.contains_key(&plan.id) {
                    return Err(format!("Fixture plan identity collision: {}", plan.id));
                }
                if plan.status == PlanStatus::Active {
                    if let Some(team) = &plan.team_id {
                        if !teams.insert(team)
                            || index.values().any(|row| {
                                row.status == PlanStatus::Active
                                    && row.team_id.as_ref() == Some(team)
                            })
                        {
                            return Err(format!("Fixture active plan Team collision: {team}"));
                        }
                    }
                }
                super::text_bounds::validate_plan(plan).map_err(|error| error.to_string())?;
            }
        }
        let rows = store_db::PlanDatabase::prepare_fixture_plans(plans)
            .map_err(|error| error.to_string())?;
        let committed = commit_desk()?;
        self.database
            .persist_fixture_plans(plans, &rows)
            .map_err(|error| {
                format!(
                    "Fixture stage=plans desk=committed plans=uncertain; do not replay: {error}"
                )
            })?;
        for plan in plans {
            self.note_index(plan);
            super::search_index::enqueue_plan_import(&turn, plan);
        }
        publish(committed)
    }

    pub(super) fn import_payload_in_writer(
        &self,
        turn: &MutationWriterTurn<'_>,
        plan: &mut Plan,
        now_ms: u64,
    ) -> std::io::Result<()> {
        turn.assert_store(self);
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
        #[cfg(test)]
        if let Some(hook) = self.persistence_hook.read().clone() {
            hook(&plan.title);
        }
        let owned = self
            .specs
            .read()
            .values()
            .find(|spec| spec.lock().plan_id.as_deref() == Some(plan.id.as_str()))
            .map(|spec| spec.lock().clone());
        let receipt = self
            .database
            .import_plan_rows(plan, owned.as_ref())
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        if let Some(receipt) = receipt {
            self.publish_hot_receipt(turn, receipt);
            super::search_index::enqueue_plan_import(turn, plan);
            #[cfg(test)]
            self.persist_test_sidecars(plan, owned.as_ref())?;
        }
        Ok(())
    }

    pub fn save_spec(&self, spec: &mut Spec, now_ms: u64) -> std::io::Result<()> {
        let writer_turn = self.mutation_writer.enter();
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
        let path = self.persist_spec_in_writer(&writer_turn, &staged)?;
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
        let sidecar_reload_blocked = {
            let writer = self.mutation_writer.state.lock();
            writer.next_ticket != writer.serving_ticket
        };
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
        #[cfg(not(test))]
        let _turn = self.mutation_writer.enter();
        // Refresh only index metadata from the committed SQL row.
        let row = self
            .database
            .load_index_row(id)?
            .ok_or(PlanError::NotFound)?;
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
