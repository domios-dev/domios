use super::super::{MutationWriterTurn, PlanError, PlanStore};
use crate::plan::Plan;

impl PlanStore {
    pub fn with_plan<T, F>(&self, id: &str, now_ms: u64, f: F) -> Result<T, PlanError>
    where
        F: FnOnce(&mut Plan) -> Result<T, PlanError>,
    {
        let writer_turn = self.mutation_writer.enter();
        Ok(self
            .with_plan_if_changed_in_writer(&writer_turn, id, now_ms, |plan| f(plan).map(Some))?
            .expect("unconditional plan mutation returns a value"))
    }

    pub(in crate::service_plan) fn with_plan_in_writer<T, F>(
        &self,
        turn: &MutationWriterTurn<'_>,
        id: &str,
        now_ms: u64,
        f: F,
    ) -> Result<T, PlanError>
    where
        F: FnOnce(&mut Plan) -> Result<T, PlanError>,
    {
        Ok(self
            .with_plan_if_changed_in_writer(turn, id, now_ms, |plan| f(plan).map(Some))?
            .expect("unconditional plan mutation returns a value"))
    }

    pub(in crate::service_plan) fn with_plan_if_changed<T, F>(
        &self,
        id: &str,
        now_ms: u64,
        f: F,
    ) -> Result<Option<T>, PlanError>
    where
        F: FnOnce(&mut Plan) -> Result<Option<T>, PlanError>,
    {
        let writer_turn = self.mutation_writer.enter();
        self.with_plan_if_changed_in_writer(&writer_turn, id, now_ms, f)
    }

    fn with_plan_if_changed_in_writer<T, F>(
        &self,
        turn: &MutationWriterTurn<'_>,
        id: &str,
        now_ms: u64,
        f: F,
    ) -> Result<Option<T>, PlanError>
    where
        F: FnOnce(&mut Plan) -> Result<Option<T>, PlanError>,
    {
        turn.assert_store(self);
        // Load (cache or SQL) before the plan lock; the held handle pins it in the cache for the whole turn.
        let arc = self.plan_arc(id)?;
        let mut staged = arc.lock().clone();
        let previous = staged.clone();
        let Some(out) = f(&mut staged)? else {
            return Ok(None);
        };
        let existing_ids: std::collections::HashSet<_> =
            previous.tasks.iter().map(|task| task.id.as_str()).collect();
        for task in &staged.tasks {
            if !existing_ids.contains(task.id.as_str()) {
                codeterm_types::plan::work_facet::validate_work_facet(
                    staged.tag_facets.iter().map(|facet| facet.key.as_str()),
                    task.tags.iter().map(String::as_str),
                )
                .map_err(PlanError::WorkFacetViolation)?;
            }
        }
        staged.updated_at = now_ms;
        if staged.canonical_project_path.trim().is_empty() {
            staged.canonical_project_path = staged.project_path.clone();
        }
        super::super::store_index::record_lifecycle_events(&mut staged, Some(&previous), now_ms);
        super::super::tag_model::normalize_plan(&mut staged);
        crate::service_plan::text_bounds::validate_plan(&staged)?;
        if let Err(error) = self.persist_plan_and_spec_in_writer(turn, &staged) {
            // Memory must never differ from disk: the next read reloads the stored row.
            self.plan_cache.evict(id);
            return Err(PlanError::Io(error.to_string()));
        }
        let committed = {
            let mut guard = arc.lock();
            *guard = staged;
            guard.clone()
        };
        self.plan_cache.commit(id, &arc);
        self.record_committed_snapshot(&committed);
        self.note_index(&committed);
        self.reset_bound_tabs(&committed);
        Ok(Some(out))
    }
}
