use super::*;

impl PlanStore {
    pub fn add_task(&self, plan_id: &str, new: NewTask, now_ms: u64) -> Result<String, PlanError> {
        self.add_task_with_external_refs(plan_id, new, &[], now_ms)
    }

    pub fn add_task_with_external_refs(
        &self,
        plan_id: &str,
        new: NewTask,
        external_refs: &[String],
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let mut new = new;
        crate::service_plan::text_bounds::validate_new_task_text(&new)?;
        new.personality = new
            .personality
            .as_deref()
            .map(crate::service_plan::worker_identity::validate_executor_name)
            .transpose()
            .map_err(PlanError::InvalidExecutorName)?;
        let external_refs =
            crate::service_plan::external_refs::canonicalize_external_refs(external_refs)?;
        crate::service_plan::text_bounds::validate_external_refs(&external_refs)?;
        let tid = self.mint_id("task")?;
        let assigned_tab_id = new
            .assigned_tab_id
            .clone()
            .or_else(|| new.lane.tab_id.clone());
        let mut lane = normalize_lane_metadata(new.lane);
        if lane.tab_id.is_none() {
            lane.tab_id = assigned_tab_id.clone();
        }
        let key = self.issue_task_keys(plan_id, 1)?.into_iter().next();
        let task = Task {
            files: new.files.validated().map_err(PlanError::InvalidPlanText)?,
            id: tid.clone(),
            name: new.name,
            kind: new.kind,
            model: new.model,
            speed: None,
            model_rationale: new.model_rationale,
            acceptance_criteria: new.acceptance_criteria,
            external_refs,
            test_plan: new.test_plan,
            qa_plan: new.qa_plan,
            correctness: new.correctness,
            status: TaskStatus::Pending,
            review_stage: Default::default(),
            qa_state: None,
            review_round: 0,
            review_oracle: None,
            review_evidence: vec![],
            key,
            review_override: None,
            review_audit: vec![],
            status_label: None,
            priority: new.priority.unwrap_or_default(),
            created_at: now_ms,
            updated_at: now_ms,
            status_changed_at: now_ms,
            handle: new.handle,
            tags: new.tags,
            section_ids: new.section_ids,
            body: new.body,
            role_guidance: new.role_guidance,
            executor_role: new.executor_role,
            personality: new.personality,
            personality_provenance: new.personality_provenance,
            competition: new.competition,
            intent: new.intent,
            execution_intent: new.execution_intent,
            parent_task_id: new.parent_task_id,
            depends_on: normalized_dependency_ids(&new.depends_on),
            assigned_tab_id,
            lane,
            taken_by_role: None,
            peer: None,
            group: new.group,
            canvas: Default::default(),
            milestones: vec![],
            execution_attempts: vec![],
            pause_reason: None,
            interruption: Default::default(),
            attachments: vec![],
        };
        crate::service_plan::text_bounds::validate_task_text(&task)?;
        let turn = self.mutation_writer.enter();
        let receipt = self
            .database
            .insert_tasks(plan_id, vec![task], None, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(tid)
    }
}
