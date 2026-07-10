use crate::plan::{Plan, Section, Spec};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RelatedGroup {
    pub edge_type: String,
    pub units: Vec<RelatedUnit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelatedUnit {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub removed: bool,
}

pub fn related_units(plans: &[Plan], specs: &[Spec], id: &str) -> Vec<RelatedGroup> {
    let mut groups = Vec::new();
    let Some(plan) = plans.iter().find(|plan| {
        plan.id == id
            || section_contains_any(&plan.sections, id)
            || plan.tasks.iter().any(|task| task.id == id)
    }) else {
        return groups;
    };

    if let Some(task) = plan.tasks.iter().find(|task| task.id == id) {
        let sections = task
            .section_ids
            .iter()
            .map(|section_id| {
                find_section(&plan.sections, section_id)
                    .map(|section| unit(section.id.clone(), "plan_section", section.title.clone()))
                    .unwrap_or_else(|| removed(section_id, "plan_section"))
            })
            .collect();
        groups.push(RelatedGroup {
            edge_type: "task_sections".into(),
            units: sections,
        });

        let children = plan
            .tasks
            .iter()
            .filter(|candidate| candidate.parent_task_id.as_deref() == Some(&task.id))
            .map(|task| unit(task.id.clone(), "task", task.name.clone()))
            .collect();
        groups.push(RelatedGroup {
            edge_type: "subtasks".into(),
            units: children,
        });
    } else if section_contains_any(&plan.sections, id) {
        let tasks = plan
            .tasks
            .iter()
            .filter(|task| task.section_ids.iter().any(|section_id| section_id == id))
            .map(|task| unit(task.id.clone(), "task", task.name.clone()))
            .collect();
        groups.push(RelatedGroup {
            edge_type: "section_tasks".into(),
            units: tasks,
        });

        let spec_links = plan
            .spec_links
            .iter()
            .filter(|link| link.plan_section_id == id)
            .map(|link| {
                specs
                    .iter()
                    .find(|spec| spec.id == link.spec_id)
                    .and_then(|spec| find_section(&spec.sections, &link.spec_section_id))
                    .map(|section| unit(section.id.clone(), "spec_section", section.title.clone()))
                    .unwrap_or_else(|| removed(&link.spec_section_id, "spec_section"))
            })
            .collect();
        groups.push(RelatedGroup {
            edge_type: "spec_links".into(),
            units: spec_links,
        });
    }

    let lineage = plan
        .lineage
        .iter()
        .map(|link| {
            plans
                .iter()
                .find(|candidate| candidate.id == link.to)
                .map(|plan| unit(plan.id.clone(), "plan", plan.title.clone()))
                .unwrap_or_else(|| removed(&link.to, "plan"))
        })
        .collect();
    groups.push(RelatedGroup {
        edge_type: "lineage".into(),
        units: lineage,
    });
    groups
}

fn unit(id: String, kind: &str, title: String) -> RelatedUnit {
    RelatedUnit {
        id,
        kind: kind.into(),
        title,
        removed: false,
    }
}

fn removed(id: &str, kind: &str) -> RelatedUnit {
    RelatedUnit {
        id: id.into(),
        kind: kind.into(),
        title: "removed".into(),
        removed: true,
    }
}

fn find_section<'a>(sections: &'a [Section], id: &str) -> Option<&'a Section> {
    for section in sections {
        if section.id == id {
            return Some(section);
        }
        if let Some(found) = find_section(&section.subsections, id) {
            return Some(found);
        }
    }
    None
}

fn section_contains(section: &Section, id: &str) -> bool {
    section.id == id || section.subsections.iter().any(|s| section_contains(s, id))
}

fn section_contains_any(sections: &[Section], id: &str) -> bool {
    sections.iter().any(|section| section_contains(section, id))
}
