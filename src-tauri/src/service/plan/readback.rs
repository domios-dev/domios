use super::{PlanError, PlanStore};
use crate::plan::{Task, TaskPriority, TaskStatus};

pub(in crate::service::plan) fn pending_after_verified(
    result: Result<(), PlanError>,
    verified: Result<(), PlanError>,
) -> Result<(), PlanError> {
    match result {
        Ok(()) => verified,
        Err(err @ PlanError::WritePending { .. }) => match verified {
            Ok(()) => Err(err),
            Err(readback) => Err(readback),
        },
        Err(err) => Err(err),
    }
}

fn parse_task_status(raw: &str) -> Result<TaskStatus, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into())).map_err(|_| PlanError::BadStatus)
}

fn parse_task_priority(raw: &str) -> Result<TaskPriority, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into()))
        .map_err(|_| PlanError::BadPriority)
}

impl PlanStore {
    fn task_readback_error(operation: &str, message: impl Into<String>) -> PlanError {
        PlanError::ReadbackMismatch {
            operation: operation.into(),
            message: message.into(),
        }
    }

    fn read_task_for_verification(&self, plan_id: &str, task_id: &str) -> Result<Task, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        plan.tasks
            .into_iter()
            .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
            .ok_or(PlanError::TaskNotFound)
    }

    pub(in crate::service::plan) fn verify_task_added(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<(), PlanError> {
        let task = self.read_task_for_verification(plan_id, task_id)?;
        if task.id == task_id {
            Ok(())
        } else {
            Err(Self::task_readback_error(
                "task_add",
                format!("expected task id {task_id}, got {}", task.id),
            ))
        }
    }

    pub(in crate::service::plan) fn verify_task_closed(
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

    #[allow(clippy::too_many_arguments)]
    pub(in crate::service::plan) fn verify_task_updated(
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
}
