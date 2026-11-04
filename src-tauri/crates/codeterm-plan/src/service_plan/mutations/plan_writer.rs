use super::super::{MutationWriterTurn, PlanError, PlanStore};
use crate::plan::Plan;

impl PlanStore {
    pub(in crate::service_plan) fn commit_hot_batch(
        &self,
        plan_id: &str,
        request: super::super::batch::BatchRequest,
        now_ms: u64,
    ) -> Result<super::super::batch::BatchResult, PlanError> {
        let turn = self.mutation_writer.enter();
        let (result, receipt) = self.database.commit_hot_batch(plan_id, request, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(result)
    }
    pub(in crate::service_plan) fn with_task_rows<T>(
        &self,
        plan_id: &str,
        selector: &str,
        history: super::super::store_db::hot_write::TaskHistory,
        now_ms: u64,
        change: impl FnOnce(&mut super::super::store_db::hot_write::TaskRowEdit) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        let turn = self.mutation_writer.enter();
        let (result, receipt) = self
            .database
            .with_task_rows(plan_id, selector, history, now_ms, change)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(result)
    }
    pub(in crate::service_plan) fn commit_hot_changes(
        &self,
        id: &str,
        changes: Vec<super::super::store_db::hot_write::HotChange>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let turn = self.mutation_writer.enter();
        let receipt = self.database.commit_hot_changes(id, changes, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(())
    }

    pub(in crate::service_plan) fn change_attempt<T>(
        &self,
        plan_id: &str,
        task_id: &str,
        target: super::super::store_db::hot_write::AttemptTarget<'_>,
        now_ms: u64,
        change: impl FnOnce(
            Option<&mut crate::plan::TaskExecutionAttempt>,
        ) -> Result<Option<T>, PlanError>,
    ) -> Result<Option<T>, PlanError> {
        let turn = self.mutation_writer.enter();
        let (result, receipt) = self
            .database
            .change_attempt(plan_id, task_id, target, now_ms, change)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(result)
    }

    pub(in crate::service_plan) fn publish_hot_receipt(
        &self,
        turn: &MutationWriterTurn<'_>,
        receipt: super::super::store_db::hot_write::CommittedRows,
    ) {
        turn.assert_store(self);
        #[cfg(test)]
        super::super::store_db::hot_write::record_work(&receipt);
        if receipt.header_rows == 0 {
            return;
        }
        self.clear_committed_snapshot();
        self.plan_cache.evict(&receipt.plan_id);
        if let Some(row) = self.index.write().get_mut(&receipt.plan_id) {
            row.updated_at = receipt.updated_at;
        }
        if let Some(root) = receipt.index_root {
            super::super::search_index::enqueue_plan_time(
                turn,
                root,
                receipt.plan_id.clone(),
                receipt.updated_at,
            );
        }
        if let Some(counter) = self.turn_counter.read().clone() {
            for tab in &receipt.reset_tabs {
                counter.reset(tab);
            }
        }
        for delta in receipt.search {
            super::super::search_index::enqueue_milestone_index(turn, delta);
        }
        for delta in receipt.task_search {
            super::super::search_index::enqueue_task_index(turn, delta);
        }
        log::debug!("[plan.store] hot commit tasks={} attempts={} milestones={} strides={} decoded_bytes={} serialized_bytes={} projected_text_bytes={} hashed_bytes={}", receipt.task_rows, receipt.attempts, receipt.milestones, receipt.strides, receipt.decoded_bytes, receipt.serialized_bytes, receipt.projected_text_bytes, receipt.hashed_bytes);
    }

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
