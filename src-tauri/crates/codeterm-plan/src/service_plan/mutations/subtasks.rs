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
            prepared.push((self.mint_id("task"), child));
        }
        let keys = self.issue_task_keys(plan_id, prepared.len());
        self.with_plan(plan_id, now_ms, |plan| {
            let parent = plan
                .tasks
                .iter()
                .find(|task| task.id == parent_id)
                .ok_or(PlanError::TaskNotFound)?;
            if parent.parent_task_id.is_some()
                || matches!(&parent.status, TaskStatus::Done | TaskStatus::Cancelled)
            {
                return Err(PlanError::BadMode);
            }
            if plan.status == PlanStatus::Archived
                || matches!(&plan.mode, PlanMode::Paused | PlanMode::Complete)
            {
                return Err(PlanError::BadMode);
            }
            let mut tasks = Vec::with_capacity(prepared.len());
            for ((id, child), key) in prepared.into_iter().zip(keys) {
                let depends_on = validate_task_dependencies(plan, &id, &child.depends_on)?;
                let mut task = Task {
                    id,
                    name: child.name,
                    kind: child.kind,
                    model: child.model,
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
                    key: Some(key),
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
                if task.section_ids.is_empty() {
                    task.section_ids = super::section_links::auto_section_ids_for_task(plan, &task);
                }
                tasks.push(task);
            }
            let ids = tasks.iter().map(|task| task.id.clone()).collect();
            plan.tasks.extend(tasks);
            Ok(ids)
        })
    }
}
