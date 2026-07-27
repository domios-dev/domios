use super::*;

impl PlanStore {
    pub(super) fn index_entry(plan: &Plan, file_path: PathBuf) -> IndexEntry {
        IndexEntry {
            plan_id: plan.id.clone(),
            file_path,
            canonical_project_path: if plan.canonical_project_path.trim().is_empty() {
                plan.project_path.clone()
            } else {
                plan.canonical_project_path.clone()
            },
            owner: plan.owner.clone(),
            team_id: plan.team_id.clone(),
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
        let entry = Self::spec_index_entry(&committed, path);
        self.spec_index.write().insert(committed.id.clone(), entry);
        Ok(())
    }

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
                let wp = self
                    .persist_plan_and_spec(&plan)
                    .map_err(|e| PlanError::Io(e.to_string()))?;
                self.note_plan_write(&plan, &wp);
            }
            plan
        };
        paths::normalize_plan_paths(&mut plan, project_root);
        tag_model::normalize_plan(&mut plan);
        // Co-located spec.yaml rides with its plan (startup scan + reconcile).
        if let Some(dir) = path.parent() {
            let spec_yaml = dir.join("spec.yaml");
            if spec_yaml.is_file() {
                self.load_spec_file_into_index(spec_yaml, project_root)?;
            }
        }
        self.commit_loaded_plan(plan)?;
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
        self.commit_loaded_spec(spec)
    }

    pub fn scan_project(&self, project_path: &Path) -> Result<PathBuf, PlanError> {
        let project_root = paths::project_root(project_path);
        let import_root = project_root.to_string_lossy().replace('\\', "/");
        if self.database.legacy_imported(&import_root)? {
            return Ok(project_root);
        }
        let mut import_error = None;
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
                    import_error.get_or_insert(e);
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
                    import_error.get_or_insert(e);
                }
            }
        }
        if let Some(error) = import_error {
            return Err(error);
        }
        self.database
            .mark_legacy_imported(&import_root, unix_time_millis())?;
        Ok(project_root)
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
        let staged = {
            let guard = arc.lock();
            if guard.updated_at <= plan.updated_at {
                plan.clone()
            } else {
                guard.clone()
            }
        };
        *plan = staged.clone();
        let path = self.persist_plan_and_spec(&staged)?;
        {
            let mut guard = arc.lock();
            *guard = staged.clone();
        }
        self.note_plan_write(&staged, &path);
        self.index
            .write()
            .insert(staged.id.clone(), Self::index_entry(&staged, path));
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
        self.note_spec_write(&staged, &path);
        self.spec_index
            .write()
            .insert(staged.id.clone(), Self::spec_index_entry(&staged, path));
        Ok(())
    }

    pub fn reload_from_disk(&self, id: &str) -> Result<(), PlanError> {
        #[cfg(test)]
        let sidecar_reload_blocked = self
            .plans
            .read()
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
        let plan = self.database.load_plan(id)?.ok_or(PlanError::NotFound)?;
        let committed = self.upsert_plan_arc(plan);
        let path = self.database.path();
        self.index
            .write()
            .insert(committed.id.clone(), Self::index_entry(&committed, path));
        Ok(())
    }
}

fn unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
