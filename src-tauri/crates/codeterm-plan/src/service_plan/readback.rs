use super::{PlanError, PlanStore, TaskLaneUpdate};
use crate::plan::{
    Plan, PlanMode, PlanStatus, PlannerRef, Task, TaskLaneMetadata, TaskPriority, TaskStatus,
};
use std::cell::RefCell;

thread_local! {
    static COMMITTED_SNAPSHOT: RefCell<Option<Plan>> = const { RefCell::new(None) };
}

pub(in crate::service_plan) fn pending_after_verified(
    result: Result<(), PlanError>,
    verified: Result<(), PlanError>,
    _committed_id: Option<String>,
) -> Result<(), PlanError> {
    pending_after_readback(result, verified)
}

pub(in crate::service_plan) fn pending_after_readback<T>(
    result: Result<(), PlanError>,
    readback: Result<T, PlanError>,
) -> Result<T, PlanError> {
    match result {
        Ok(()) => readback,
        Err(PlanError::WritePending { .. }) => match readback {
            Ok(value) => Ok(value),
            Err(readback) => Err(readback),
        },
        Err(err) => Err(err),
    }
}

pub(in crate::service_plan) struct TaskAddExpectation {
    name: String,
    kind: String,
    model: Option<String>,
    model_rationale: Option<String>,
    acceptance_criteria: Vec<String>,
    test_plan: Option<String>,
    qa_plan: Option<String>,
    correctness: Option<String>,
    body: String,
    role_guidance: Option<String>,
    executor_role: Option<String>,
    intent: Option<String>,
    parent_task_id: Option<String>,
    pub(in crate::service_plan) depends_on: Vec<String>,
    assigned_tab_id: Option<String>,
    lane: TaskLaneMetadata,
    group: Option<String>,
    priority: TaskPriority,
    section_ids: Option<Vec<String>>,
}

fn parse_task_status(raw: &str) -> Result<TaskStatus, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into())).map_err(|_| PlanError::BadStatus)
}

fn parse_task_priority(raw: &str) -> Result<TaskPriority, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into()))
        .map_err(|_| PlanError::BadPriority)
}

fn parse_plan_mode(raw: &str) -> Result<PlanMode, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into())).map_err(|_| PlanError::BadMode)
}

fn normalized_lane_value(value: &Option<String>) -> Option<String> {
    value.as_deref().and_then(|raw| {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

impl PlanStore {
    pub(in crate::service_plan) fn task_add_expectation(
        task: &Task,
        verify_sections: bool,
    ) -> TaskAddExpectation {
        TaskAddExpectation {
            name: task.name.clone(),
            kind: task.kind.clone(),
            model: task.model.clone(),
            model_rationale: task.model_rationale.clone(),
            acceptance_criteria: task.acceptance_criteria.clone(),
            test_plan: task.test_plan.clone(),
            qa_plan: task.qa_plan.clone(),
            correctness: task.correctness.clone(),
            body: task.body.clone(),
            role_guidance: task.role_guidance.clone(),
            executor_role: task.executor_role.clone(),
            intent: task.intent.clone(),
            parent_task_id: task.parent_task_id.clone(),
            depends_on: task.depends_on.clone(),
            assigned_tab_id: task.assigned_tab_id.clone(),
            lane: task.lane.clone(),
            group: task.group.clone(),
            priority: task.priority.clone(),
            section_ids: verify_sections.then(|| task.section_ids.clone()),
        }
    }

    pub(in crate::service_plan) fn task_readback_error(
        operation: &str,
        message: impl Into<String>,
    ) -> PlanError {
        PlanError::ReadbackMismatch {
            operation: operation.into(),
            message: message.into(),
        }
    }

    fn plan_readback_error(operation: &str, message: impl Into<String>) -> PlanError {
        PlanError::ReadbackMismatch {
            operation: operation.into(),
            message: message.into(),
        }
    }

    // Re-reading after the plan lock drops races concurrent writers; verify the snapshot committed under the lock.
    pub(in crate::service_plan) fn record_committed_snapshot(&self, plan: &Plan) {
        COMMITTED_SNAPSHOT.with(|cell| *cell.borrow_mut() = Some(plan.clone()));
    }

    pub(in crate::service_plan) fn committed_plan_for(&self, plan_id: &str) -> Option<Plan> {
        match COMMITTED_SNAPSHOT.with(|cell| cell.borrow_mut().take()) {
            Some(plan) if plan.id == plan_id => Some(plan),
            _ => self.get(plan_id),
        }
    }

    pub(in crate::service_plan) fn verify_plan_updated(
        &self,
        plan_id: &str,
        title: Option<&str>,
        status: Option<PlanStatus>,
        mode: Option<&str>,
    ) -> Result<(), PlanError> {
        let plan = self
            .committed_plan_for(plan_id)
            .ok_or(PlanError::NotFound)?;
        let fail = |field: &str| {
            Self::plan_readback_error(
                "plan_update",
                format!("{field} did not match for {plan_id}"),
            )
        };
        if title.is_some_and(|expected| plan.title != expected) {
            return Err(fail("title"));
        }
        if status.is_some_and(|expected| plan.status != expected) {
            return Err(fail("status"));
        }
        if let Some(raw_mode) = mode {
            let expected = parse_plan_mode(raw_mode)?;
            if plan.mode != expected {
                return Err(fail("mode"));
            }
        }
        Ok(())
    }

    pub(in crate::service_plan) fn verify_planner_ref(
        &self,
        plan_id: &str,
        expected: &Option<PlannerRef>,
    ) -> Result<(), PlanError> {
        let plan = self
            .committed_plan_for(plan_id)
            .ok_or(PlanError::NotFound)?;
        if &plan.planner != expected {
            return Err(Self::plan_readback_error(
                "plan_update",
                format!("planner did not match for {plan_id}"),
            ));
        }
        Ok(())
    }

    pub(in crate::service_plan) fn read_task_for_verification(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<Task, PlanError> {
        let plan = self
            .committed_plan_for(plan_id)
            .ok_or(PlanError::NotFound)?;
        plan.tasks
            .into_iter()
            .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
            .ok_or(PlanError::TaskNotFound)
    }

    pub(in crate::service_plan) fn verify_task_added(
        &self,
        plan_id: &str,
        task_id: &str,
        expected: &TaskAddExpectation,
    ) -> Result<(), PlanError> {
        let task = self.read_task_for_verification(plan_id, task_id)?;
        let fail = |field: &str| {
            Self::task_readback_error("task_add", format!("{field} did not match for {}", task.id))
        };
        if task.id != task_id {
            return Err(Self::task_readback_error(
                "task_add",
                format!("expected task id {task_id}, got {}", task.id),
            ));
        }
        if task.name != expected.name {
            return Err(fail("name"));
        }
        if task.kind != expected.kind {
            return Err(fail("type"));
        }
        if task.model != expected.model {
            return Err(fail("model"));
        }
        if task.model_rationale != expected.model_rationale {
            return Err(fail("why"));
        }
        if task.acceptance_criteria != expected.acceptance_criteria {
            return Err(fail("acceptance_criteria"));
        }
        if task.test_plan != expected.test_plan {
            return Err(fail("test_plan"));
        }
        if task.qa_plan != expected.qa_plan {
            return Err(fail("qa_plan"));
        }
        if task.correctness != expected.correctness {
            return Err(fail("correctness"));
        }
        if task.body != expected.body {
            return Err(fail("body"));
        }
        if task.role_guidance != expected.role_guidance {
            return Err(fail("role_guidance"));
        }
        if task.executor_role != expected.executor_role {
            return Err(fail("executor_role"));
        }
        if task.intent != expected.intent {
            return Err(fail("intent"));
        }
        if task.parent_task_id != expected.parent_task_id {
            return Err(fail("parent_task_id"));
        }
        if task.depends_on != expected.depends_on {
            return Err(fail("depends_on"));
        }
        if task.assigned_tab_id != expected.assigned_tab_id {
            return Err(fail("assigned_tab_id"));
        }
        if task.lane != expected.lane {
            return Err(fail("lane"));
        }
        if task.group != expected.group {
            return Err(fail("group"));
        }
        if task.priority != expected.priority {
            return Err(fail("priority"));
        }
        // Handle is server-normalized (slugged, collision-suffixed); comparing the
        // raw requested handle against the committed one is a false-positive source.
        if expected
            .section_ids
            .as_ref()
            .is_some_and(|sections| task.section_ids != *sections)
        {
            return Err(fail("sections"));
        }
        Ok(())
    }

    pub(in crate::service_plan) fn verify_task_closed(
        &self,
        plan_id: &str,
        task_id: &str,
        result: Option<&str>,
    ) -> Result<(), PlanError> {
        let task = self.read_task_for_verification(plan_id, task_id)?;
        if task.status != TaskStatus::Done {
            return Err(Self::task_readback_error(
                "task_close",
                format!(
                    "expected done status for {}, got {:?}",
                    task.id, task.status
                ),
            ));
        }
        if let Some(expected) = result {
            if !task
                .milestones
                .iter()
                .any(|milestone| milestone.text == expected)
            {
                return Err(Self::task_readback_error(
                    "task_close",
                    format!("missing close milestone for {}", task.id),
                ));
            }
        }
        Ok(())
    }

    pub(in crate::service_plan) fn verify_task_milestone_added(
        &self,
        plan_id: &str,
        task_id: &str,
        milestone_id: &str,
        kind: &str,
        text: &str,
    ) -> Result<(), PlanError> {
        let task = self.read_task_for_verification(plan_id, task_id)?;
        let Some(milestone) = task
            .milestones
            .iter()
            .find(|milestone| milestone.id == milestone_id)
        else {
            return Err(Self::task_readback_error(
                "task_record",
                format!("missing milestone {milestone_id} for {}", task.id),
            ));
        };
        if milestone.kind != kind {
            return Err(Self::task_readback_error(
                "task_record",
                format!("milestone kind did not match for {}", task.id),
            ));
        }
        if milestone.text != text {
            return Err(Self::task_readback_error(
                "task_record",
                format!("milestone text did not match for {}", task.id),
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::service_plan) fn verify_task_updated(
        &self,
        plan_id: &str,
        task_id: &str,
        name: Option<&str>,
        kind: Option<&str>,
        model: Option<Option<&str>>,
        model_rationale: Option<Option<&str>>,
        status: Option<&str>,
        status_label: Option<Option<&str>>,
        priority: Option<&str>,
        body: Option<&str>,
        intent: Option<Option<&str>>,
        executor_role: Option<Option<&str>>,
        section_ids: Option<&[String]>,
        parent_task_id: Option<Option<&str>>,
        group: Option<Option<&str>>,
        handle: Option<&str>,
    ) -> Result<(), PlanError> {
        let task = self.read_task_for_verification(plan_id, task_id)?;
        let fail = |field: &str| {
            Self::task_readback_error(
                "task_update",
                format!("{field} did not match for {}", task.id),
            )
        };
        if name.is_some_and(|expected| task.name != expected) {
            return Err(fail("name"));
        }
        if kind.is_some_and(|expected| task.kind != expected) {
            return Err(fail("type"));
        }
        if model.is_some_and(|expected| task.model.as_deref() != expected) {
            return Err(fail("model"));
        }
        if model_rationale.is_some_and(|expected| task.model_rationale.as_deref() != expected) {
            return Err(fail("why"));
        }
        if let Some(raw_status) = status {
            let expected = parse_task_status(raw_status)?;
            if task.status != expected {
                return Err(fail("status"));
            }
        }
        if status_label.is_some_and(|expected| task.status_label.as_deref() != expected) {
            return Err(fail("status_label"));
        }
        if let Some(raw_priority) = priority {
            let expected = parse_task_priority(raw_priority)?;
            if task.priority != expected {
                return Err(fail("priority"));
            }
        }
        if body.is_some_and(|expected| task.body != expected) {
            return Err(fail("body"));
        }
        if intent.is_some_and(|expected| task.intent.as_deref() != expected) {
            return Err(fail("intent"));
        }
        if executor_role.is_some_and(|expected| task.executor_role.as_deref() != expected) {
            return Err(fail("executor_role"));
        }
        if section_ids.is_some_and(|expected| task.section_ids != expected) {
            return Err(fail("sections"));
        }
        if parent_task_id.is_some_and(|expected| task.parent_task_id.as_deref() != expected) {
            return Err(fail("parent_task_id"));
        }
        if group.is_some_and(|expected| task.group.as_deref() != expected) {
            return Err(fail("group"));
        }
        if handle.is_some_and(|expected| task.handle.as_deref() != Some(expected)) {
            return Err(fail("handle"));
        }
        Ok(())
    }

    pub(in crate::service_plan) fn verify_task_lane_updated(
        &self,
        plan_id: &str,
        task_id: &str,
        expected: &TaskLaneUpdate,
    ) -> Result<(), PlanError> {
        let task = self.read_task_for_verification(plan_id, task_id)?;
        let fail = |field: &str| {
            Self::task_readback_error(
                "task_lane_update",
                format!("{field} did not match for {}", task.id),
            )
        };
        if let Some(value) = &expected.tab_id {
            let expected = normalized_lane_value(value);
            if task.lane.tab_id != expected {
                return Err(fail("lane.tab_id"));
            }
        }
        if let Some(value) = &expected.workspace_id {
            let expected = normalized_lane_value(value);
            if task.lane.workspace_id != expected {
                return Err(fail("lane.workspace_id"));
            }
        }
        if let Some(value) = &expected.workspace_name {
            let expected = normalized_lane_value(value);
            if task.lane.workspace_name != expected {
                return Err(fail("lane.workspace_name"));
            }
        }
        if let Some(value) = &expected.worktree_path {
            let expected = normalized_lane_value(value);
            if task.lane.worktree_path != expected {
                return Err(fail("lane.worktree_path"));
            }
        }
        if let Some(value) = &expected.branch {
            let expected = normalized_lane_value(value);
            if task.lane.branch != expected {
                return Err(fail("lane.branch"));
            }
        }
        if let Some(value) = &expected.base {
            let expected = normalized_lane_value(value);
            if task.lane.base != expected {
                return Err(fail("lane.base"));
            }
        }
        if let Some(value) = &expected.destination {
            let expected = normalized_lane_value(value);
            if task.lane.destination != expected {
                return Err(fail("lane.destination"));
            }
        }
        if let Some(value) = &expected.cleanup_status {
            let expected = normalized_lane_value(value);
            if task.lane.cleanup_status != expected {
                return Err(fail("lane.cleanup_status"));
            }
        }
        if let Some(value) = &expected.cleanup_hints {
            let mut expected = value.clone();
            expected.retain(|hint| !hint.trim().is_empty());
            if task.lane.cleanup_hints != expected {
                return Err(fail("lane.cleanup_hints"));
            }
        }
        if let Some(value) = &expected.tab_id {
            let expected = normalized_lane_value(value);
            if task.assigned_tab_id.is_none() && expected.is_some() {
                return Err(fail("assigned_tab_id"));
            }
        }
        Ok(())
    }

    pub(in crate::service_plan) fn verify_task_dependencies_updated(
        &self,
        plan_id: &str,
        task_id: &str,
        depends_on: &[String],
    ) -> Result<(), PlanError> {
        let task = self.read_task_for_verification(plan_id, task_id)?;
        if task.depends_on != depends_on {
            return Err(Self::task_readback_error(
                "task_dependency_update",
                format!("depends_on did not match for {}", task.id),
            ));
        }
        Ok(())
    }

    pub(in crate::service_plan) fn verify_task_removed(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<(), PlanError> {
        let plan = self
            .committed_plan_for(plan_id)
            .ok_or(PlanError::NotFound)?;
        if plan
            .tasks
            .iter()
            .any(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
        {
            return Err(Self::task_readback_error(
                "task_remove",
                format!("task {task_id} still exists after removal"),
            ));
        }
        Ok(())
    }

    pub(in crate::service_plan) fn verify_task_removal_rejected(
        &self,
        plan_id: &str,
        task_id: &str,
        dependent_task_ids: &[String],
    ) -> Result<(), PlanError> {
        let plan = self
            .committed_plan_for(plan_id)
            .ok_or(PlanError::NotFound)?;
        if !plan
            .tasks
            .iter()
            .any(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
        {
            return Err(Self::task_readback_error(
                "task_remove",
                format!("task {task_id} was removed despite dependency rejection"),
            ));
        }
        for dependent_task_id in dependent_task_ids {
            let Some(task) = plan.tasks.iter().find(|task| task.id == *dependent_task_id) else {
                return Err(Self::task_readback_error(
                    "task_remove",
                    format!("dependent task {dependent_task_id} missing after rejection"),
                ));
            };
            if !task
                .depends_on
                .iter()
                .any(|dependency_id| dependency_id == task_id)
            {
                return Err(Self::task_readback_error(
                    "task_remove",
                    format!(
                        "dependent task {dependent_task_id} no longer references {task_id} after rejection"
                    ),
                ));
            }
        }
        Ok(())
    }

    pub(in crate::service_plan) fn verify_task_assigned(
        &self,
        plan_id: &str,
        task_id: &str,
        tab: &str,
        peer: Option<&str>,
    ) -> Result<(), PlanError> {
        let task = self.read_task_for_verification(plan_id, task_id)?;
        if task.assigned_tab_id.as_deref() != Some(tab) {
            return Err(Self::task_readback_error(
                "task_assign",
                format!("assigned_tab_id did not match for {}", task.id),
            ));
        }
        if task.lane.tab_id.as_deref() != Some(tab) {
            return Err(Self::task_readback_error(
                "task_assign",
                format!("lane.tab_id did not match for {}", task.id),
            ));
        }
        if task.peer.as_deref() != peer {
            return Err(Self::task_readback_error(
                "task_assign",
                format!("peer did not match for {}", task.id),
            ));
        }
        Ok(())
    }

    pub(in crate::service_plan) fn verify_task_assignment(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: Option<&str>,
        taken_by_role: Option<&str>,
    ) -> Result<(), PlanError> {
        let task = self.read_task_for_verification(plan_id, task_id)?;
        if task.assigned_tab_id.as_deref() != tab_id {
            return Err(Self::task_readback_error(
                "task_assignment",
                format!("assigned_tab_id did not match for {}", task.id),
            ));
        }
        if task.taken_by_role.as_deref() != taken_by_role {
            return Err(Self::task_readback_error(
                "task_assignment",
                format!("taken_by_role did not match for {}", task.id),
            ));
        }
        if tab_id.is_none() && task.peer.is_some() {
            return Err(Self::task_readback_error(
                "task_assignment",
                format!("peer was not cleared for {}", task.id),
            ));
        }
        Ok(())
    }
}
