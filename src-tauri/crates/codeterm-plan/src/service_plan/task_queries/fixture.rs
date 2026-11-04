use super::{TaskDetail, TaskLabel, TaskSection};
use crate::plan::{Plan, Section, Task};
use crate::service_plan::state::dependency_waits;
use crate::service_plan::PlanStore;
use std::collections::HashSet;

fn collect_sections(sections: &[Section], ids: &HashSet<&str>, out: &mut Vec<TaskSection>) {
    for section in sections {
        if ids.contains(section.id.as_str()) {
            out.push(TaskSection {
                id: section.id.clone(),
                title: section.title.clone(),
            });
        }
        collect_sections(&section.subsections, ids, out);
    }
}

fn task_sections(sections: &[Section], task: &Task) -> Vec<TaskSection> {
    let ids = task.section_ids.iter().map(String::as_str).collect();
    let mut out = vec![];
    collect_sections(sections, &ids, &mut out);
    out
}

fn task_label(task: &Task) -> TaskLabel {
    TaskLabel {
        id: task.id.clone(),
        display_key: task.display_key(),
        name: task.name.clone(),
        status: task.status.clone(),
    }
}

impl PlanStore {
    pub fn inspect_task_for_test(&self, task_id: &str) -> Option<TaskDetail> {
        let Ok(target) = self.task_authority_target_projection(task_id) else {
            return None;
        };
        let plan = self.get(&target.plan_id)?;
        detail_from_plan(&plan, &target.task_id)
    }
}

fn detail_from_plan(plan: &Plan, task_id: &str) -> Option<TaskDetail> {
    let task = crate::plan::task_key::find_task(&plan.tasks, task_id)?;
    let parent = task.parent_task_id.as_ref().and_then(|parent_id| {
        plan.tasks
            .iter()
            .find(|candidate| candidate.id == *parent_id)
            .map(task_label)
    });
    let dependency_waits = dependency_waits(&plan, task);
    Some(TaskDetail {
        plan_id: plan.id.clone(),
        plan_title: plan.title.clone(),
        plan_owner: plan.owner.clone(),
        display_key: task.display_key(),
        sections: task_sections(&plan.sections, task),
        parent,
        siblings: plan
            .tasks
            .iter()
            .filter(|candidate| {
                candidate.id != task.id && candidate.parent_task_id == task.parent_task_id
            })
            .map(task_label)
            .collect(),
        subtasks: plan
            .tasks
            .iter()
            .filter(|candidate| candidate.parent_task_id.as_deref() == Some(&task.id))
            .map(task_label)
            .collect(),
        presentation: crate::service_plan::state::task_projection(&plan, task).presentation,
        dependency_waits,
        task: task.clone(),
    })
}
