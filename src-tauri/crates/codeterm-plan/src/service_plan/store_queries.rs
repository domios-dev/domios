use super::*;
use crate::service_plan::paths;
use crate::service_plan::plan_match;

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
        self.index.read().values().find_map(|row| {
            (row.status == PlanStatus::Active && row.project_path == project_path)
                .then(|| (row.id.clone(), row.title.clone()))
        })
    }

    /// `None` when the plan is unknown or quarantined (the quarantine is logged).
    pub fn get(&self, id: &str) -> Option<Plan> {
        match self.try_get(id) {
            Ok(plan) => Some(plan),
            Err(PlanError::NotFound) => None,
            Err(error) => {
                log::warn!("[plan.store] {error}; source left unchanged");
                None
            }
        }
    }

    /// Plan ids opened by id since the last call.
    #[cfg(any(test, feature = "test-support"))]
    pub fn take_plan_touches(&self) -> Vec<String> {
        std::mem::take(&mut *self.plan_touches.lock())
    }

    /// `None` when the plan is unknown; the inner value is the plan's Team binding.
    pub fn plan_team_id(&self, id: &str) -> Option<Option<String>> {
        self.index.read().get(id).map(|row| row.team_id.clone())
    }

    pub fn live_active_plan_ids(&self) -> Vec<String> {
        self.database
            .live_active_plan_ids()
            .unwrap_or_else(|error| {
                log::warn!("[plan.store] live active plan query failed: {error}");
                Vec::new()
            })
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
        let plans = self.plans_matching(&PlanMatch::SpecLink(id.to_string()));
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
        let candidates = PlanMatch::active().and(TaskMatch::assigned(
            tab_id,
            &plan_match::NON_TERMINAL_TASK_STATUSES,
        ));
        for plan in self.plans_matching(&candidates) {
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

    pub fn open_attempt_task_for_tab(
        &self,
        tab_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
    ) -> Option<(String, String, String)> {
        self.find_open_attempt_task_for_tab(tab_id, session_id, session_binding_nonce, true)
    }

    pub fn open_attempt_task_for_effort_for_tab(
        &self,
        tab_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
    ) -> Option<(String, String, String)> {
        self.find_open_attempt_task_for_tab(tab_id, session_id, session_binding_nonce, false)
    }

    fn find_open_attempt_task_for_tab(
        &self,
        tab_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
        require_unknown_model: bool,
    ) -> Option<(String, String, String)> {
        if tab_id.trim().is_empty()
            || session_id.trim().is_empty()
            || session_binding_nonce.trim().is_empty()
        {
            return None;
        }
        let mut matched = None;
        let candidates = PlanMatch::active().and(
            AttemptMatch {
                session_id: Some(session_id.to_string()),
                binding_nonce: Some(session_binding_nonce.to_string()),
                task_statuses: Some(plan_match::NON_TERMINAL_TASK_STATUSES.to_vec()),
                ..AttemptMatch::executor(tab_id, AttemptState::Open)
            }
            .into_match(),
        );
        for plan in self.plans_matching(&candidates) {
            if plan.status != PlanStatus::Active {
                continue;
            }
            for task in plan
                .tasks
                .iter()
                .filter(|task| !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled))
            {
                for attempt in task.execution_attempts.iter().filter(|attempt| {
                    attempt.executor_tab_id == tab_id
                        && attempt.closed_at.is_none()
                        && attempt.retired_at.is_none()
                        && (!require_unknown_model || attempt.session_model.is_none())
                        && attempt.session_id.as_deref() == Some(session_id)
                        && attempt.session_binding_nonce.as_deref() == Some(session_binding_nonce)
                }) {
                    if matched.is_some() {
                        return None;
                    }
                    matched = Some((plan.id.clone(), task.id.clone(), attempt.id.clone()));
                }
            }
        }
        matched
    }

    pub fn report_pending_task_binding_for_tab(&self, tab_id: &str) -> Option<(String, String)> {
        if tab_id.trim().is_empty() {
            return None;
        }
        let candidates = PlanMatch::active()
            .and(PlanMatch::Mode(PlanMode::Execution))
            .and(TaskMatch::assigned(
                tab_id,
                &[
                    TaskStatus::Pending,
                    TaskStatus::InProgress,
                    TaskStatus::Blocked,
                ],
            ));
        for plan in self.plans_matching(&candidates) {
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
        let candidates = PlanMatch::active()
            .and(PlanMatch::Mode(PlanMode::Execution))
            .and(PlanMatch::owner(tab_id));
        for plan in self.plans_matching(&candidates) {
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
        let candidates = PlanMatch::active().and(PlanMatch::Planner(tab_id.to_string()));
        for plan in self.plans_matching(&candidates) {
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
        let bound = PlanMatch::active().and(PlanMatch::Any(vec![
            PlanMatch::owner(tab_id).and(PlanMatch::Mode(PlanMode::Execution)),
            PlanMatch::Planner(tab_id.to_string()),
        ]));
        for plan in self.plans_matching(&bound) {
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
        let candidates = PlanMatch::active().and(
            AttemptMatch {
                task_statuses: Some(plan_match::NON_TERMINAL_TASK_STATUSES.to_vec()),
                ..AttemptMatch::executor(tab_id, AttemptState::Unclosed)
            }
            .into_match(),
        );
        self.plans_matching(&candidates)
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
        let candidates = PlanMatch::active().and(TaskMatch::assigned(
            tab_id,
            &plan_match::NON_TERMINAL_TASK_STATUSES,
        ));
        self.plans_matching(&candidates)
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

    /// Built from SQL aggregates over the stored rows; no plan is loaded.
    pub fn list_index(&self) -> Vec<IndexEntry> {
        self.database.list_index_entries().unwrap_or_else(|error| {
            log::warn!("[plan.store] plan index query failed: {error}");
            Vec::new()
        })
    }

    pub fn list_spec_index(&self) -> Vec<SpecIndexEntry> {
        self.spec_index.read().values().cloned().collect()
    }

    /// Ids of the plans whose stored rows match, newest first; no plan is loaded.
    pub fn plan_ids_matching(&self, predicate: &PlanMatch) -> Vec<String> {
        self.database
            .plan_ids_matching(predicate)
            .unwrap_or_else(|error| {
                log::warn!("[plan.store] plan match query failed: {error}");
                Vec::new()
            })
    }

    /// Loads, through the cache, only the plans whose stored rows match, newest first.
    pub fn plans_matching(&self, predicate: &PlanMatch) -> Vec<Plan> {
        self.plan_ids_matching(predicate)
            .iter()
            .filter_map(|id| self.get(id))
            .collect()
    }

    /// The stored rows of the matching plans, read from SQL around the cache: for a
    /// caller holding a plan or mux lock, which taking a cached plan's lock could deadlock.
    pub fn stored_plans_matching(&self, predicate: &PlanMatch) -> Vec<Plan> {
        self.plan_ids_matching(predicate)
            .iter()
            .filter_map(|id| match self.load_plan_from_sql(id) {
                Ok(plan) => plan,
                Err(error) => {
                    log::warn!("[plan.store] {error}; source left unchanged");
                    None
                }
            })
            .collect()
    }

    /// Every plan, one at a time from SQL and never cached: the session export only.
    pub fn for_each_plan_uncached(&self, mut visit: impl FnMut(Plan)) {
        let mut ids: Vec<String> = self.index.read().keys().cloned().collect();
        ids.sort();
        for id in ids {
            match self.load_plan_from_sql(&id) {
                Ok(Some(plan)) => visit(plan),
                Ok(None) => {}
                Err(error) => log::warn!("[plan.store] {error}; source left unchanged"),
            }
        }
    }

    /// Every plan id the index knows; no plan is loaded.
    pub fn plan_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.index.read().keys().cloned().collect();
        ids.sort();
        ids
    }

    /// `(plan_id, project root)` per plan: the canonical path when set, else the project path.
    pub fn plan_roots(&self) -> Vec<(String, String)> {
        self.database.plan_roots().unwrap_or_else(|error| {
            log::warn!("[plan.store] plan root query failed: {error}");
            Vec::new()
        })
    }

    /// One text field (a JSON path such as `$.lane.worktree_path`) of every task row holding it.
    pub fn task_field_values(&self, path: &str) -> Vec<TaskFieldRow> {
        self.database
            .task_field_values(path)
            .unwrap_or_else(|error| {
                log::warn!("[plan.store] task field query failed: {error}");
                Vec::new()
            })
    }

    /// One text field of every attempt row holding it.
    pub fn attempt_field_values(&self, path: &str) -> Vec<AttemptFieldRow> {
        self.database
            .attempt_field_values(path)
            .unwrap_or_else(|error| {
                log::warn!("[plan.store] attempt field query failed: {error}");
                Vec::new()
            })
    }

    pub(super) fn task_ref_rows(&self) -> Vec<TaskRefRow> {
        self.database.task_ref_rows().unwrap_or_else(|error| {
            log::warn!("[plan.store] task reference query failed: {error}");
            Vec::new()
        })
    }

    pub(super) fn section_ref_rows(&self) -> Vec<SectionRefRow> {
        self.database.section_ref_rows().unwrap_or_else(|error| {
            log::warn!("[plan.store] section reference query failed: {error}");
            Vec::new()
        })
    }

    pub(super) fn loop_rows(&self) -> Vec<(String, String, String)> {
        self.database.loop_rows().unwrap_or_else(|error| {
            log::warn!("[plan.store] loop query failed: {error}");
            Vec::new()
        })
    }

    /// Active execution plans with unfinished work: the only plans reminder ticks act on.
    pub fn reminder_candidate_plans(&self) -> Vec<Plan> {
        self.plans_matching(&reminder_candidates())
    }

    /// Reminder candidates this tab owns or holds an assignment in.
    pub fn reminder_candidate_plans_for_tab(&self, tab_id: &str) -> Vec<Plan> {
        if tab_id.trim().is_empty() {
            return Vec::new();
        }
        self.plans_matching(&reminder_candidates().and(PlanMatch::Any(vec![
            PlanMatch::owner(tab_id),
            TaskMatch::assigned(tab_id, &plan_match::NON_TERMINAL_TASK_STATUSES),
        ])))
    }

    /// Plans the tab owns or is the bound Planner of.
    pub fn plans_owned_or_planned_by(&self, tab_id: &str) -> Vec<Plan> {
        self.plans_matching(&PlanMatch::Any(vec![
            PlanMatch::owner(tab_id),
            PlanMatch::Planner(tab_id.to_string()),
        ]))
    }

    /// Plans bound to the Team or holding work of one of its member tabs.
    pub fn team_close_plans(&self, team_id: &str, member_tab_ids: &[String]) -> Vec<Plan> {
        self.plans_matching(&team_close_candidates(team_id, member_tab_ids))
    }

    /// A plan bound to the Team, the active one first; read from the index, nothing loaded.
    pub fn plan_id_for_team(&self, team_id: &str) -> Option<String> {
        let index = self.index.read();
        let mut bound: Vec<_> = index
            .values()
            .filter(|row| row.team_id.as_deref() == Some(team_id))
            .collect();
        bound.sort_by(|left, right| {
            (right.status == PlanStatus::Active)
                .cmp(&(left.status == PlanStatus::Active))
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        bound.first().map(|row| row.id.clone())
    }

    /// Every stored plan for test oracles; production paths load by match only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn all_plans_for_test(&self) -> Vec<Plan> {
        let mut plans = Vec::new();
        self.for_each_plan_uncached(|plan| plans.push(plan));
        plans
    }

    /// Plans whose stored rows hold a finished task with a lane that may still
    /// need settling. A superset: callers confirm each one with `plan_matches`.
    pub fn plan_ids_with_unsettled_lanes(&self) -> Vec<String> {
        self.database
            .plans_with_unsettled_lanes()
            .unwrap_or_else(|error| {
                log::warn!("[plan.store] unsettled lane query failed: {error}");
                Vec::new()
            })
    }

    /// Judges one plan under its lock without cloning it; unknown is `false`.
    pub fn plan_matches(&self, id: &str, matches: impl FnOnce(&Plan) -> bool) -> bool {
        self.plan_arc(id).is_ok_and(|plan| matches(&plan.lock()))
    }

    pub fn delete(&self, id: &str) -> std::io::Result<()> {
        let _writer_turn = self.mutation_writer.enter();
        self.delete_in_writer(id, self.get(id))
    }

    pub(super) fn delete_in_writer(
        &self,
        id: &str,
        indexed_plan: Option<Plan>,
    ) -> std::io::Result<()> {
        if let Some(plan) = indexed_plan {
            let specs = self
                .list_specs_full()
                .into_iter()
                .filter(|spec| spec.plan_id.as_deref() == Some(id))
                .collect::<Vec<_>>();
            super::search_index::remove_plan_from_index(plan, &specs)
                .map_err(std::io::Error::other)?;
        }
        self.database
            .delete_plan(id, unix_time_millis())
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        self.forget_plan(id);
        super::attachments::remove_dir_if_exists(self.plan_data_dir(id))
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        Ok(())
    }

    /// Drop a plan from the in-memory index + search units without touching disk.
    /// The plan's owned spec (1:1, co-located) lives and dies with it.
    pub(super) fn forget_plan(&self, id: &str) {
        self.plan_cache.evict(id);
        self.index.write().remove(id);
        self.forget_owned_spec(id);
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

fn reminder_candidates() -> PlanMatch {
    PlanMatch::active()
        .and(PlanMatch::Mode(PlanMode::Execution))
        .and(TaskMatch::with_status(&[
            TaskStatus::Pending,
            TaskStatus::InProgress,
            TaskStatus::InReview,
            TaskStatus::Blocked,
        ]))
}

pub(super) fn team_close_candidates(team_id: &str, member_tab_ids: &[String]) -> PlanMatch {
    let tabs = member_tab_ids.to_vec();
    PlanMatch::Any(vec![
        PlanMatch::team(team_id),
        PlanMatch::Task(TaskMatch {
            assigned_tabs: Some(tabs.clone()),
            ..Default::default()
        }),
        PlanMatch::Task(TaskMatch {
            lane_tabs: Some(tabs.clone()),
            ..Default::default()
        }),
        PlanMatch::Attempt(AttemptMatch {
            executor_tabs: Some(tabs),
            ..Default::default()
        }),
    ])
}

fn unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests;
