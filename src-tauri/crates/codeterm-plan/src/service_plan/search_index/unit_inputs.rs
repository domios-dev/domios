use super::{stable_id, Unit};
use crate::plan::TaskStatus;

pub(super) struct UnitParent<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub scope: &'a str,
    pub status: &'a str,
}

pub(super) struct TaskUnitInput<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub status: &'a TaskStatus,
    pub updated_at: u64,
    pub body: &'a str,
    pub acceptance_criteria: &'a [String],
    pub external_refs: Vec<&'a str>,
}

pub(super) struct StrideUnitInput<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub text: &'a str,
    pub ts: u64,
}

impl UnitParent<'_> {
    pub(super) fn header(&self, titles: &str, updated_at: u64) -> Unit {
        Unit {
            id: stable_id(self.id, "plan", self.id),
            kind: "plan",
            title: self.title.into(),
            body: format!("Plan \"{}\"\n{titles}", self.title),
            scope: self.scope.into(),
            plan_id: Some(self.id.into()),
            spec_id: None,
            unit_id: self.id.into(),
            unit_title: self.title.into(),
            parent_title: self.title.into(),
            status: Some(self.status.into()),
            updated_at,
        }
    }

    pub(super) fn section(
        &self,
        id: &str,
        title: &str,
        body: &str,
        ordinal: usize,
        total: usize,
        updated_at: u64,
    ) -> Unit {
        Unit {
            id: stable_id(self.id, "plan_section", id),
            kind: "plan_section",
            title: format!("{} section {title}", self.title),
            body: format!(
                "Plan \"{}\" · section {ordinal}/{total}: {title}\n{body}",
                self.title
            ),
            scope: self.scope.into(),
            plan_id: Some(self.id.into()),
            spec_id: None,
            unit_id: id.into(),
            unit_title: title.into(),
            parent_title: self.title.into(),
            status: Some(self.status.into()),
            updated_at,
        }
    }

    pub(super) fn task(&self, task: TaskUnitInput<'_>) -> Unit {
        Unit {
            id: stable_id(self.id, "task", task.id),
            kind: "task",
            title: format!("{} task {}", self.title, task.name),
            body: format!(
                "Plan \"{}\" · task: {}\n{}\n{}\n{}",
                self.title,
                task.name,
                task.acceptance_criteria.join("\n"),
                task.external_refs.join("\n"),
                task.body
            ),
            scope: self.scope.into(),
            plan_id: Some(self.id.into()),
            spec_id: None,
            unit_id: task.id.into(),
            unit_title: task.name.into(),
            parent_title: self.title.into(),
            status: Some(format!("{}:{}", self.status, task.status.key())),
            updated_at: task.updated_at,
        }
    }

    pub(super) fn stride(
        &self,
        task_name: &str,
        task_status: &str,
        stride: StrideUnitInput<'_>,
    ) -> Unit {
        Unit {
            id: stable_id(self.id, "stride", stride.id),
            kind: "stride",
            title: format!("{task_name} stride {}", stride.kind),
            body: format!(
                "Plan \"{}\" · task \"{task_name}\" · stride({}): {}",
                self.title, stride.kind, stride.text
            ),
            scope: self.scope.into(),
            plan_id: Some(self.id.into()),
            spec_id: None,
            unit_id: stride.id.into(),
            unit_title: stride.kind.into(),
            parent_title: task_name.into(),
            status: Some(format!("{}:{task_status}", self.status)),
            updated_at: stride.ts,
        }
    }
}

#[cfg(test)]
mod tests;
