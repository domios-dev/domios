use super::*;

impl PlanStore {
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

    pub fn bind_turn_counter(&self, counter: Arc<reminders::TurnCounter>) {
        *self.turn_counter.write() = Some(counter);
    }

    pub(super) fn reset_bound_panes(&self, plan: &Plan) {
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
        let _writer_turn = self.mutation_writer.enter();
        let indexed_plan = self.get(id);
        self.database
            .delete_plan(id, unix_time_millis())
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        self.forget_plan(id);
        if let Some(plan) = indexed_plan {
            super::search_index::remove_plan_from_index(plan);
        }
        super::attachments::remove_dir_if_exists(self.plan_data_dir(id))
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        Ok(())
    }

    /// Drop a plan from the in-memory index + search units without touching disk.
    /// The plan's owned spec (1:1, co-located) lives and dies with it.
    pub(super) fn forget_plan(&self, id: &str) -> (Option<Plan>, Option<PathBuf>) {
        let plan = self
            .plans
            .write()
            .remove(id)
            .map(|plan| plan.lock().clone());
        let entry_path = self.index.write().remove(id).map(|e| e.file_path);
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
}

fn unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
