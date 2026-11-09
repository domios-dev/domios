use super::*;

impl PlanStore {
    pub fn add_child_tasks_atomic(
        &self,
        plan_id: &str,
        parent_id: &str,
        children: Vec<NewTask>,
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        if children.is_empty() {
            return Err(PlanError::BadMode);
        }
        let mut prepared = Vec::with_capacity(children.len());
        for mut child in children {
            child.personality = child
                .personality
                .as_deref()
                .map(crate::service_plan::worker_identity::validate_executor_name)
                .transpose()
                .map_err(PlanError::InvalidExecutorName)?;
            child.parent_task_id = Some(parent_id.to_string());
            prepared.push((self.mint_id("task")?, child));
        }
        let keys = self.issue_task_keys(plan_id, prepared.len())?;
        let mut tasks = Vec::with_capacity(prepared.len());
        let keys = keys.into_iter().map(Some).chain(std::iter::repeat(None));
        for ((id, child), key) in prepared.into_iter().zip(keys) {
            let depends_on = normalized_dependency_ids(&child.depends_on);
            let task = Task {
                files: child
                    .files
                    .validated()
                    .map_err(PlanError::InvalidPlanText)?,
                id,
                name: child.name,
                kind: child.kind,
                model: child.model,
                speed: None,
                model_rationale: child.model_rationale,
                acceptance_criteria: child.acceptance_criteria,
                external_refs: vec![],
                test_plan: child.test_plan,
                qa_plan: child.qa_plan,
                correctness: child.correctness,
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
                priority: child.priority.unwrap_or_default(),
                created_at: now_ms,
                updated_at: now_ms,
                status_changed_at: now_ms,
                handle: child.handle,
                tags: child.tags,
                section_ids: child.section_ids,
                body: child.body,
                role_guidance: child.role_guidance,
                executor_role: child.executor_role,
                personality: child.personality,
                personality_provenance: child.personality_provenance,
                competition: child.competition,
                intent: child.intent,
                execution_intent: child.execution_intent,
                parent_task_id: Some(parent_id.to_string()),
                depends_on,
                assigned_tab_id: None,
                lane: Default::default(),
                taken_by_role: None,
                peer: None,
                group: child.group,
                canvas: Default::default(),
                milestones: vec![],
                execution_attempts: vec![],
                pause_reason: None,
                interruption: Default::default(),
                attachments: vec![],
            };
            tasks.push(task);
        }
        let ids = tasks.iter().map(|task| task.id.clone()).collect();
        let turn = self.mutation_writer.enter();
        let receipt = self
            .database
            .insert_tasks(plan_id, tasks, Some(parent_id), now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(ids)
    }
}
