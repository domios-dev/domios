use super::{NewTask, PlanError, TaskLaneUpdate};
use crate::plan::{ExternalRef, Task, TaskCompetitionMetadata, TaskLaneMetadata};
use serde_json::Value;

pub(super) const MAX_PLAN_TEXT_BYTES: usize = 256 * 1024;
const MAX_PLAN_LABEL_CHARS: usize = 256;
const MAX_PLAN_TEXT_FIELDS: usize = 1024;
const MAX_STORED_PLAN_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_STORED_PLAN_TEXT_FIELDS: usize = 65536;

#[derive(Default)]
struct TextBudget {
    bytes: usize,
    fields: usize,
}

impl TextBudget {
    fn add(&mut self, field: &str, value: &str) -> Result<(), PlanError> {
        self.fields += 1;
        self.bytes = self.bytes.saturating_add(value.len());
        if self.fields > MAX_PLAN_TEXT_FIELDS {
            return Err(PlanError::InvalidPlanText(format!(
                "task text has more than {MAX_PLAN_TEXT_FIELDS} fields"
            )));
        }
        if self.bytes > MAX_PLAN_TEXT_BYTES {
            return Err(PlanError::InvalidPlanText(format!(
                "task text exceeds {MAX_PLAN_TEXT_BYTES} UTF-8 bytes"
            )));
        }
        if value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        {
            return Err(PlanError::InvalidPlanText(format!(
                "{field} contains a control character"
            )));
        }
        Ok(())
    }
}

pub(super) fn validate_plan_text(field: &str, value: &str) -> Result<(), PlanError> {
    let mut budget = TextBudget::default();
    budget.add(field, value)
}

pub(super) fn validate_plan(plan: &crate::plan::Plan) -> Result<(), PlanError> {
    validate_plan_label("plan title", &plan.title)?;
    for section in &plan.sections {
        validate_section(section)?;
    }
    for task in &plan.tasks {
        validate_task_text(task)?;
    }
    let value = serde_json::to_value(plan)
        .map_err(|error| PlanError::InvalidPlanText(error.to_string()))?;
    validate_stored_plan_value(&value, &mut StoredPlanTextBudget::default())
}

pub(super) fn validate_spec(spec: &crate::plan::Spec) -> Result<(), PlanError> {
    validate_plan_label("spec title", &spec.title)?;
    for section in &spec.sections {
        validate_section(section)?;
    }
    let value = serde_json::to_value(spec)
        .map_err(|error| PlanError::InvalidPlanText(error.to_string()))?;
    validate_stored_plan_value(&value, &mut StoredPlanTextBudget::default())
}

pub(super) fn validate_plan_label(field: &str, value: &str) -> Result<(), PlanError> {
    validate_plan_text(field, value)?;
    validate_label_shape(field, value)
}

fn validate_label_shape(field: &str, value: &str) -> Result<(), PlanError> {
    if value.trim().is_empty() {
        return Err(PlanError::InvalidPlanText(format!(
            "{field} must not be empty"
        )));
    }
    if value.chars().count() > MAX_PLAN_LABEL_CHARS {
        return Err(PlanError::InvalidPlanText(format!(
            "{field} exceeds {MAX_PLAN_LABEL_CHARS} characters"
        )));
    }
    if value
        .chars()
        .any(|character| matches!(character, '\n' | '\r' | '\t' | '\u{2028}' | '\u{2029}'))
    {
        return Err(PlanError::InvalidPlanText(format!(
            "{field} must be one line"
        )));
    }
    Ok(())
}

pub(super) fn validate_new_task_text(new: &NewTask) -> Result<(), PlanError> {
    let mut budget = TextBudget::default();
    add_label(&mut budget, "task name", &new.name)?;
    add_label(&mut budget, "task type", &new.kind)?;
    add(&mut budget, "model", new.model.as_deref())?;
    add(
        &mut budget,
        "routing rationale",
        new.model_rationale.as_deref(),
    )?;
    add(&mut budget, "personality", new.personality.as_deref())?;
    add_many(
        &mut budget,
        "acceptance criterion",
        &new.acceptance_criteria,
    )?;
    add(&mut budget, "test plan", new.test_plan.as_deref())?;
    add(&mut budget, "QA plan", new.qa_plan.as_deref())?;
    add(&mut budget, "correctness", new.correctness.as_deref())?;
    add(&mut budget, "task body", Some(&new.body))?;
    add(&mut budget, "role guidance", new.role_guidance.as_deref())?;
    add(&mut budget, "executor role", new.executor_role.as_deref())?;
    add(&mut budget, "intent", new.intent.as_deref())?;
    add(&mut budget, "parent task id", new.parent_task_id.as_deref())?;
    add(&mut budget, "task group", new.group.as_deref())?;
    add(&mut budget, "task handle", new.handle.as_deref())?;
    add_many(&mut budget, "section id", &new.section_ids)?;
    add_many(&mut budget, "dependency task id", &new.depends_on)?;
    add(
        &mut budget,
        "assigned tab id",
        new.assigned_tab_id.as_deref(),
    )?;
    add_lane(&mut budget, &new.lane)?;
    if let Some(competition) = &new.competition {
        add_competition(&mut budget, competition)?;
    }
    add_many(&mut budget, "task tag", &new.tags)
}

pub(super) fn validate_task_text(task: &Task) -> Result<(), PlanError> {
    let mut budget = TextBudget::default();
    add_label(&mut budget, "task name", &task.name)?;
    add_label(&mut budget, "task type", &task.kind)?;
    add(&mut budget, "model", task.model.as_deref())?;
    add(
        &mut budget,
        "routing rationale",
        task.model_rationale.as_deref(),
    )?;
    add(&mut budget, "personality", task.personality.as_deref())?;
    add_many(
        &mut budget,
        "acceptance criterion",
        &task.acceptance_criteria,
    )?;
    add_external_refs(&mut budget, &task.external_refs)?;
    add(&mut budget, "test plan", task.test_plan.as_deref())?;
    add(&mut budget, "QA plan", task.qa_plan.as_deref())?;
    add(&mut budget, "correctness", task.correctness.as_deref())?;
    add(&mut budget, "status label", task.status_label.as_deref())?;
    add(&mut budget, "task handle", task.handle.as_deref())?;
    add_many(&mut budget, "task tag", &task.tags)?;
    add_many(&mut budget, "section id", &task.section_ids)?;
    add(&mut budget, "task body", Some(&task.body))?;
    add(&mut budget, "role guidance", task.role_guidance.as_deref())?;
    add(&mut budget, "executor role", task.executor_role.as_deref())?;
    add(&mut budget, "intent", task.intent.as_deref())?;
    add(
        &mut budget,
        "parent task id",
        task.parent_task_id.as_deref(),
    )?;
    add_many(&mut budget, "dependency task id", &task.depends_on)?;
    add(
        &mut budget,
        "assigned tab id",
        task.assigned_tab_id.as_deref(),
    )?;
    add(&mut budget, "task group", task.group.as_deref())?;
    add_lane(&mut budget, &task.lane)?;
    if let Some(competition) = &task.competition {
        add_competition(&mut budget, competition)?;
    }
    Ok(())
}

pub(super) fn validate_external_refs(values: &[ExternalRef]) -> Result<(), PlanError> {
    let mut budget = TextBudget::default();
    add_external_refs(&mut budget, values)
}

pub(super) fn validate_lane_update(lane: &TaskLaneUpdate) -> Result<(), PlanError> {
    let mut budget = TextBudget::default();
    add(&mut budget, "lane tab id", nested_value(&lane.tab_id))?;
    add(
        &mut budget,
        "lane workspace id",
        nested_value(&lane.workspace_id),
    )?;
    add(
        &mut budget,
        "lane workspace name",
        nested_value(&lane.workspace_name),
    )?;
    add(
        &mut budget,
        "lane worktree path",
        nested_value(&lane.worktree_path),
    )?;
    add(&mut budget, "lane branch", nested_value(&lane.branch))?;
    add(&mut budget, "lane base", nested_value(&lane.base))?;
    add(
        &mut budget,
        "lane destination",
        nested_value(&lane.destination),
    )?;
    add(
        &mut budget,
        "lane cleanup status",
        nested_value(&lane.cleanup_status),
    )?;
    if let Some(hints) = &lane.cleanup_hints {
        add_many(&mut budget, "lane cleanup hint", hints)?;
    }
    Ok(())
}

fn add_external_refs(budget: &mut TextBudget, values: &[ExternalRef]) -> Result<(), PlanError> {
    for value in values {
        budget.add("external reference repository", &value.repository)?;
        budget.add("external reference URL", &value.canonical_url)?;
    }
    Ok(())
}

fn add_label(budget: &mut TextBudget, field: &str, value: &str) -> Result<(), PlanError> {
    budget.add(field, value)?;
    validate_label_shape(field, value)
}

fn add(budget: &mut TextBudget, field: &str, value: Option<&str>) -> Result<(), PlanError> {
    if let Some(value) = value {
        budget.add(field, value)?;
    }
    Ok(())
}

fn nested_value(value: &Option<Option<String>>) -> Option<&str> {
    value.as_ref().and_then(Option::as_deref)
}

fn validate_section(section: &crate::plan::Section) -> Result<(), PlanError> {
    validate_plan_label("section title", &section.title)?;
    validate_plan_text("section body", &section.body)?;
    for tag in &section.tags {
        validate_plan_label("section tag", tag)?;
    }
    if let Some(handle) = section.handle.as_deref() {
        validate_plan_label("section handle", handle)?;
    }
    for subsection in &section.subsections {
        validate_section(subsection)?;
    }
    Ok(())
}

#[derive(Default)]
struct StoredPlanTextBudget {
    bytes: usize,
    fields: usize,
}

fn validate_stored_plan_value(
    value: &Value,
    budget: &mut StoredPlanTextBudget,
) -> Result<(), PlanError> {
    match value {
        Value::String(text) => {
            budget.fields += 1;
            budget.bytes = budget.bytes.saturating_add(text.len());
            if text.len() > MAX_PLAN_TEXT_BYTES {
                return Err(PlanError::InvalidPlanText(format!(
                    "plan text exceeds {MAX_PLAN_TEXT_BYTES} UTF-8 bytes"
                )));
            }
            if budget.fields > MAX_STORED_PLAN_TEXT_FIELDS {
                return Err(PlanError::InvalidPlanText(format!(
                    "plan has more than {MAX_STORED_PLAN_TEXT_FIELDS} text fields"
                )));
            }
            if budget.bytes > MAX_STORED_PLAN_TEXT_BYTES {
                return Err(PlanError::InvalidPlanText(format!(
                    "plan text exceeds {MAX_STORED_PLAN_TEXT_BYTES} total UTF-8 bytes"
                )));
            }
            if text
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
            {
                return Err(PlanError::InvalidPlanText(
                    "plan text contains a control character".into(),
                ));
            }
            Ok(())
        }
        Value::Array(values) => {
            for item in values {
                validate_stored_plan_value(item, budget)?;
            }
            Ok(())
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                validate_stored_plan_value(&Value::String(key.clone()), budget)?;
                validate_stored_plan_value(value, budget)?;
            }
            Ok(())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
    }
}

fn add_many(budget: &mut TextBudget, field: &str, values: &[String]) -> Result<(), PlanError> {
    for value in values {
        budget.add(field, value)?;
    }
    Ok(())
}

fn add_lane(budget: &mut TextBudget, lane: &TaskLaneMetadata) -> Result<(), PlanError> {
    add(budget, "lane tab id", lane.tab_id.as_deref())?;
    add(budget, "lane workspace id", lane.workspace_id.as_deref())?;
    add(
        budget,
        "lane workspace name",
        lane.workspace_name.as_deref(),
    )?;
    add(budget, "lane worktree path", lane.worktree_path.as_deref())?;
    add(budget, "lane branch", lane.branch.as_deref())?;
    add(budget, "lane base", lane.base.as_deref())?;
    add(budget, "lane destination", lane.destination.as_deref())?;
    add(
        budget,
        "lane cleanup status",
        lane.cleanup_status.as_deref(),
    )?;
    add_many(budget, "lane cleanup hint", &lane.cleanup_hints)
}

fn add_competition(
    budget: &mut TextBudget,
    competition: &TaskCompetitionMetadata,
) -> Result<(), PlanError> {
    add_many(budget, "competition model", &competition.models)?;
    add(
        budget,
        "competition judge hint",
        competition.judge_hint.as_deref(),
    )
}
