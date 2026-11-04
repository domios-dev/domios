use super::*;
use crate::service_plan::paths;
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

    #[cfg(any(test, feature = "test-support"))]
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

    /// Compact exact-plan admission metadata from one committed statement.
    pub fn plan_team_binding_metadata(
        &self,
        id: &str,
    ) -> Result<Option<(PlanStatus, Option<String>, bool)>, PlanError> {
        self.database.plan_team_binding_metadata(id)
    }

    pub fn active_plan_team_bindings(&self) -> Result<Vec<(String, Option<String>)>, PlanError> {
        self.database.active_plan_team_bindings()
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

    pub fn active_owned_plan_with_open_work_for_tab(
        &self,
        tab_id: &str,
    ) -> Result<Option<String>, PlanError> {
        self.database.active_owned_open_plan(tab_id)
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
    pub fn list_index(&self) -> Result<Vec<IndexEntry>, PlanError> {
        self.database.plan_list_index(None, None, false, None)
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

    pub fn curator_bootstrap_plan_ids(&self) -> Result<Vec<String>, PlanError> {
        self.database.curator_bootstrap_plan_ids()
    }

    /// Every plan id the index knows; no plan is loaded.
    pub fn plan_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.index.read().keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn attempt_lane_paths(&self) -> Result<Vec<AttemptLanePathRead>, PlanError> {
        self.database.attempt_lane_paths()
    }

    pub(super) fn section_ref_rows(&self) -> Result<Vec<SectionRefRow>, PlanError> {
        self.database.section_ref_rows()
    }

    pub(super) fn loop_rows(&self) -> Result<Vec<(String, String, String)>, PlanError> {
        self.database.loop_rows()
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

    /// A SQL superset; selected lane facts make the final settlement decision.
    pub fn plan_ids_with_unsettled_lanes(&self) -> Result<Vec<String>, PlanError> {
        self.database.plans_with_unsettled_lanes()
    }

    pub fn delete(&self, id: &str) -> std::io::Result<()> {
        let turn = self.mutation_writer.enter();
        self.delete_in_writer(&turn, id, None).map(drop)
    }

    pub(super) fn delete_in_writer(
        &self,
        turn: &MutationWriterTurn<'_>,
        id: &str,
        cutoff: Option<u64>,
    ) -> std::io::Result<Option<store_db::hot_write::DeletedPlanRows>> {
        turn.assert_store(self);
        let removed = self
            .database
            .delete_plan_rows(id, cutoff, unix_time_millis())
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        if removed.is_some() || cutoff.is_none() {
            self.forget_plan(id);
        }
        if let Some(removed) = &removed {
            super::search_index::remove_plan_from_index(
                turn,
                super::search_index::PlanIndexRemoval {
                    id: id.into(),
                    root: removed.root.clone(),
                },
                &removed.specs,
            )
            .map_err(std::io::Error::other)?;
            super::attachments::remove_dir_if_exists(self.plan_data_dir(id))
                .map_err(|error| std::io::Error::other(error.to_string()))?;
        }
        Ok(removed)
    }

    /// Drop a plan from the in-memory index + search units without touching disk.
    /// The plan's owned spec (1:1, co-located) lives and dies with it.
    pub(super) fn forget_plan(&self, id: &str) {
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
