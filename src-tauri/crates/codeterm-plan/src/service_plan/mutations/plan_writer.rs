use super::super::{MutationWriterTurn, PlanError, PlanStore};

impl PlanStore {
    pub(in crate::service_plan) fn task_readback_error(
        operation: &str,
        message: impl Into<String>,
    ) -> PlanError {
        PlanError::ReadbackMismatch {
            operation: operation.into(),
            message: message.into(),
        }
    }

    pub(in crate::service_plan) fn with_review_header<T>(
        &self,
        plan_id: &str,
        task: Option<&str>,
        evidence: Option<(&str, &str, &str)>,
        now: u64,
        change: impl FnOnce(
            &mut super::super::store_db::hot_write::review_header::ReviewHeaderRows,
        ) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        let turn = self.mutation_writer.enter();
        let (result, receipt) = self
            .database
            .with_review_header(plan_id, task, evidence, now, change)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(result)
    }

    pub(in crate::service_plan) fn commit_section_change(
        &self,
        plan_id: &str,
        change: super::super::store_db::hot_write::SectionChange,
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        let turn = self.mutation_writer.enter();
        let (removed, receipt) = self.database.change_section(plan_id, change, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(removed)
    }

    pub(in crate::service_plan) fn commit_header_change(
        &self,
        plan_id: &str,
        change: super::super::store_db::hot_write::HeaderChange,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        let turn = self.mutation_writer.enter();
        let (changed, receipt) = self.database.change_header(plan_id, change, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(changed)
    }

    pub(in crate::service_plan) fn commit_hot_batch(
        &self,
        plan_id: &str,
        request: super::super::batch::BatchRequest,
        now_ms: u64,
    ) -> Result<super::super::batch::BatchResult, PlanError> {
        // Reserve outside the writer: the key port may acquire the mux lock.
        let count = request
            .ops
            .iter()
            .filter_map(|op| match op {
                super::super::batch::BatchOp::UpsertTask { handle, .. } => Some(handle.as_str()),
                _ => None,
            })
            .collect::<std::collections::HashSet<_>>()
            .len();
        let keys = self.issue_task_keys(plan_id, count)?;
        let turn = self.mutation_writer.enter();
        let (result, receipt) = self
            .database
            .commit_hot_batch(plan_id, request, keys, now_ms)?;
        #[cfg(test)]
        let metrics = super::super::store_db::hot_write::work_values(&receipt);
        self.publish_hot_receipt(&turn, receipt);
        #[cfg(test)]
        super::super::store_db::hot_write::record_work_values(metrics);
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
    pub(in crate::service_plan) fn with_task_rows_in_writer<T>(
        &self,
        turn: &MutationWriterTurn<'_>,
        plan_id: &str,
        selector: &str,
        history: super::super::store_db::hot_write::TaskHistory,
        now_ms: u64,
        change: impl FnOnce(&mut super::super::store_db::hot_write::TaskRowEdit) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        turn.assert_store(self);
        let (result, receipt) = self
            .database
            .with_task_rows(plan_id, selector, history, now_ms, change)?;
        self.publish_hot_receipt(turn, receipt);
        Ok(result)
    }

    pub(in crate::service_plan) fn with_task_rows_batch<T>(
        &self,
        plan_id: &str,
        selectors: &[String],
        history: super::super::store_db::hot_write::TaskHistory,
        now_ms: u64,
        change: impl FnMut(
            usize,
            &mut super::super::store_db::hot_write::TaskRowEdit,
        ) -> Result<Option<T>, PlanError>,
    ) -> Result<Vec<T>, PlanError> {
        let turn = self.mutation_writer.enter();
        let (result, receipt) = self
            .database
            .with_task_rows_batch(plan_id, selectors, history, now_ms, change)?;
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
        for spec in receipt.specs {
            let existing = self.specs.read().get(&spec.id).cloned();
            if let Some(existing) = existing {
                *existing.lock() = spec.clone();
            } else {
                self.specs.write().insert(
                    spec.id.clone(),
                    std::sync::Arc::new(parking_lot::Mutex::new(spec.clone())),
                );
            }
            self.spec_index.write().insert(
                spec.id.clone(),
                Self::spec_index_entry(&spec, self.database.path()),
            );
            super::super::search_index::enqueue_spec_index(turn, spec);
        }
        if receipt.header_rows == 0 {
            return;
        }
        if let Some(metadata) = receipt.plan_metadata {
            if let Some(root) = receipt.index_root.clone() {
                super::super::search_index::enqueue_plan_metadata(turn, root, metadata.clone());
            }
            self.index.write().insert(receipt.plan_id.clone(), metadata);
        } else if let Some(row) = self.index.write().get_mut(&receipt.plan_id) {
            row.updated_at = receipt.updated_at;
        }
        if let Some(root) = receipt.index_root {
            for (task_id, milestones) in receipt.removed_tasks {
                super::super::search_index::enqueue_task_removal(
                    turn,
                    root.clone(),
                    &receipt.plan_id,
                    &task_id,
                    &milestones,
                );
            }
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
        for delta in receipt.section_search {
            super::super::search_index::enqueue_section_index(turn, delta);
        }
        for created in receipt.created_plans {
            self.publish_hot_receipt(turn, created);
        }
        log::debug!("[plan.store] hot commit tasks={} attempts={} milestones={} strides={} decoded_bytes={} serialized_bytes={} projected_text_bytes={} hashed_bytes={}", receipt.task_rows, receipt.attempts, receipt.milestones, receipt.strides, receipt.decoded_bytes, receipt.serialized_bytes, receipt.projected_text_bytes, receipt.hashed_bytes);
    }
}
