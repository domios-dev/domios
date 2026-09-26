use super::*;
use crate::service_plan::paths;

impl PlanStore {
    /// codeterm-releases#56: a freshly minted Team never owns a plan, so the
    /// existence check must scope to the project instead, not the team.
    pub fn active_plan_for_project(&self, project_path: &str) -> Option<(String, String)> {
        let project_path = paths::project_root(std::path::Path::new(project_path))
            .display()
            .to_string();
        if project_path.is_empty() {
            return None;
        }
        self.plans.read().values().find_map(|arc| {
            let plan = arc.lock();
            (plan.status == PlanStatus::Active && plan.project_path == project_path)
                .then(|| (plan.id.clone(), plan.title.clone()))
        })
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

    pub fn bind_turn_counter(&self, counter: Arc<reminders::TurnCounter>) {
        *self.turn_counter.write() = Some(counter);
    }

    pub(super) fn reset_bound_tabs(&self, plan: &Plan) {
        let Some(counter) = self.turn_counter.read().clone() else {
            return;
        };
        for tab_id in plan
            .tasks
            .iter()
            .filter_map(|t| t.assigned_tab_id.as_deref())
        {
            counter.reset(tab_id);
        }
    }

    pub fn task_binding_for_tab(&self, tab_id: &str) -> Option<(String, String)> {
        if tab_id.trim().is_empty() {
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
                .find(|t| t.assigned_tab_id.as_deref() == Some(tab_id))
            {
                return Some((plan.id.clone(), task.id.clone()));
            }
        }
        None
    }

    pub fn open_attempt_task_for_tab(&self, tab_id: &str) -> Option<(String, String)> {
        if tab_id.trim().is_empty() {
            return None;
        }
        for plan in self.plans.read().values() {
            let plan = plan.lock();
            if plan.status != PlanStatus::Active {
                continue;
            }
            if let Some(task) = plan.tasks.iter().find(|task| {
                !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled)
                    && task.execution_attempts.iter().any(|attempt| {
                        attempt.executor_tab_id == tab_id
                            && attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                    })
            }) {
                return Some((plan.id.clone(), task.id.clone()));
            }
        }
        None
    }

    pub fn report_pending_task_binding_for_tab(&self, tab_id: &str) -> Option<(String, String)> {
        if tab_id.trim().is_empty() {
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
                .find(|t| t.assigned_tab_id.as_deref() == Some(tab_id))
            {
                return Some((plan.id.clone(), task.id.clone()));
            }
        }
        None
    }

    pub fn active_owned_plan_with_open_work_for_tab(&self, tab_id: &str) -> Option<String> {
        if tab_id.trim().is_empty() {
            return None;
        }
        for plan in self.plans.read().values() {
            let plan = plan.lock();
            if plan.status != PlanStatus::Active
                || plan.mode != PlanMode::Execution
                || plan.owner.tab_id.as_deref() != Some(tab_id)
            {
                continue;
            }
            if plan.tasks.iter().any(|t| t.status.counts_as_open_work()) {
                return Some(plan.id.clone());
            }
        }
        None
    }

    /// The plan this tab is the registered live Planner of, if any — a
    /// Planner binds directly, never through a task assignment.
    pub fn planner_plan_id_for_tab(&self, tab_id: &str) -> Option<String> {
        if tab_id.trim().is_empty() {
            return None;
        }
        for plan in self.plans.read().values() {
            let plan = plan.lock();
            if plan.status != PlanStatus::Active {
                continue;
            }
            if let Some(crate::plan::PlannerRef::Tab {
                tab_id: planner_tab_id,
                ..
            }) = &plan.planner
            {
                if planner_tab_id == tab_id {
                    return Some(plan.id.clone());
                }
            }
        }
        None
    }

    /// Every active plan this tab is bound to *without* a task assignment: the owner
    /// of an active execution plan, whatever its tasks' statuses, and the plan this tab
    /// is the registered live Planner of. Both readings come from the stored binding, never
    /// from UI selection. The whole list is returned rather than a first match,
    /// because more than one candidate is genuinely ambiguous and only the caller can
    /// say what an ambiguous tab should be charged to.
    pub fn taskless_plan_context_candidates_for_tab(&self, tab_id: &str) -> Vec<String> {
        if tab_id.trim().is_empty() {
            return Vec::new();
        }
        let mut candidates: Vec<String> = Vec::new();
        for plan in self.plans.read().values() {
            let plan = plan.lock();
            if plan.status != PlanStatus::Active {
                continue;
            }
            // A paused plan still costs its owner tokens, so ownership alone charges spend.
            let is_owner =
                plan.mode == PlanMode::Execution && plan.owner.tab_id.as_deref() == Some(tab_id);
            let is_planner = matches!(
                &plan.planner,
                Some(crate::plan::PlannerRef::Tab {
                    tab_id: planner_tab_id,
                    ..
                }) if planner_tab_id == tab_id
            );
            if is_owner || is_planner {
                candidates.push(plan.id.clone());
            }
        }
        candidates.sort();
        candidates.dedup();
        candidates
    }

    pub fn task_bindings_for_tab(&self, tab_id: &str) -> Vec<(String, String)> {
        if tab_id.trim().is_empty() {
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
                    .filter(move |task| {
                        task.execution_attempts.iter().any(|attempt| {
                            attempt.executor_tab_id == tab_id && attempt.closed_at.is_none()
                        })
                    })
                    .map(move |task| (plan_id.clone(), task.id))
            })
            .collect()
    }

    /// Like `task_bindings_for_tab` but also returns the task name for error messages.
    pub fn task_bindings_with_name_for_tab(&self, tab_id: &str) -> Vec<(String, String, String)> {
        if tab_id.trim().is_empty() {
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
                    .filter(move |task| task.assigned_tab_id.as_deref() == Some(tab_id))
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::plan::OrchestratorRef;

    /// Windows paths are case-insensitive on disk; a query naming the same
    /// project directory with different letter casing must still resolve.
    #[test]
    fn active_plan_for_project_matches_a_differently_cased_windows_path() {
        let root = tempfile::tempdir().unwrap();
        let project_dir = root.path().join("MyProject");
        std::fs::create_dir(&project_dir).unwrap();
        let store = PlanStore::new_for_test(&project_dir);
        store
            .create_plan_for_team(
                "Windows casing".into(),
                project_dir.display().to_string(),
                OrchestratorRef::from_tab("orch"),
                "team_case".into(),
                vec![],
                1,
            )
            .unwrap();

        let canonical = project_dir.canonicalize().unwrap().display().to_string();
        let differently_cased = canonical.replace("MyProject", "myproject");
        assert_ne!(
            differently_cased, canonical,
            "fixture path has no casing to flip"
        );

        assert!(
            store.active_plan_for_project(&differently_cased).is_some(),
            "a differently-cased query for the same directory must still resolve"
        );
    }
}
