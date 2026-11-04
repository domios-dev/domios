use super::*;
use crate::service_plan::paths;
use crate::service_plan::plan_match;
use serde_json::Value;

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

    /// Compact exact-plan admission metadata; this never loads or clones the Plan.
    pub fn plan_team_binding_metadata(
        &self,
        id: &str,
    ) -> Option<(PlanStatus, Option<String>, bool)> {
        match self.database.plan_team_binding_metadata(id) {
            Ok(metadata) => metadata,
            Err(error) => {
                log::warn!("[plan.store] {error}; source left unchanged");
                None
            }
        }
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
    ) -> Result<Option<(String, String, String)>, PlanError> {
        self.find_open_attempt_task_for_tab(tab_id, session_id, session_binding_nonce, true)
    }

    pub fn open_attempt_task_for_effort_for_tab(
        &self,
        tab_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
    ) -> Result<Option<(String, String, String)>, PlanError> {
        self.find_open_attempt_task_for_tab(tab_id, session_id, session_binding_nonce, false)
    }

    fn find_open_attempt_task_for_tab(
        &self,
        tab_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
        require_unknown_model: bool,
    ) -> Result<Option<(String, String, String)>, PlanError> {
        self.database.open_attempt_binding(
            tab_id,
            session_id,
            session_binding_nonce,
            require_unknown_model,
        )
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

    pub fn active_owned_plan_with_open_work_for_tab(
        &self,
        tab_id: &str,
    ) -> Result<Option<String>, PlanError> {
        self.database.active_owned_open_plan(tab_id)
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

    pub fn task_bindings_for_tab(&self, tab_id: &str) -> Result<Vec<(String, String)>, PlanError> {
        self.database.unclosed_attempt_bindings(tab_id)
    }

    /// Assigned nonterminal tasks, including their names for authority diagnostics.
    pub fn task_bindings_with_name_for_tab(
        &self,
        tab_id: &str,
    ) -> Result<Vec<(String, String, String)>, PlanError> {
        self.database.assigned_task_bindings(tab_id)
    }

    /// Built from SQL aggregates over the stored rows; no plan is loaded.
    pub fn list_index(&self) -> Vec<IndexEntry> {
        self.database.list_index_entries().unwrap_or_else(|error| {
            log::warn!("[plan.store] plan index query failed: {error}");
            Vec::new()
        })
    }

    pub fn task_board_page(
        &self,
        plan_id: &str,
        status: Option<&str>,
        limit: usize,
        after: Option<(u64, i64, &str)>,
    ) -> Result<Value, PlanError> {
        self.database.task_board_page(plan_id, status, limit, after)
    }

    pub fn task_detail_for_plan(&self, plan_id: &str, task_id: &str) -> Result<Value, PlanError> {
        self.database.task_detail(plan_id, task_id)
    }

    pub fn section_outline(&self, plan_id: &str) -> Result<Value, PlanError> {
        self.database.section_outline(plan_id)
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

    pub(super) fn section_ref_rows(&self) -> Result<Vec<SectionRefRow>, PlanError> {
        self.database.section_ref_rows()
    }

    pub(super) fn loop_rows(&self) -> Result<Vec<(String, String, String)>, PlanError> {
        self.database.loop_rows()
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
        for id in self.plan_ids() {
            match self.load_plan_from_sql(&id) {
                Ok(Some(plan)) => plans.push(plan),
                Ok(None) => {}
                Err(error) => log::warn!("[plan.store] {error}; source left unchanged"),
            }
        }
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
        let writer_turn = self.mutation_writer.enter();
        self.delete_in_writer(&writer_turn, id, self.get(id))
    }

    pub(super) fn delete_in_writer(
        &self,
        turn: &MutationWriterTurn<'_>,
        id: &str,
        indexed_plan: Option<Plan>,
    ) -> std::io::Result<()> {
        turn.assert_store(self);
        if let Some(plan) = indexed_plan {
            let specs = self
                .list_specs_full()
                .into_iter()
                .filter(|spec| spec.plan_id.as_deref() == Some(id))
                .collect::<Vec<_>>();
            super::search_index::remove_plan_from_index(turn, plan, &specs)
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
