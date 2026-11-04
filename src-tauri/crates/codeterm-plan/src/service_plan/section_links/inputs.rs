use super::{candidate_fields, collect_sections, SectionCandidate};
use crate::plan::Plan;

pub(in crate::service_plan) struct SectionLinkInput {
    pub sections: Vec<SectionLinkSection>,
    pub tasks: Vec<SectionLinkTask>,
}

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct SectionLinkSection {
    pub id: String,
    pub title: String,
    pub handle: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Clone, serde::Deserialize)]
pub(in crate::service_plan) struct SectionLinkTask {
    pub id: String,
    pub name: String,
    pub handle: Option<String>,
    pub group: Option<String>,
    pub section_ids: Vec<String>,
}

pub(in crate::service_plan) trait SectionLinkSource {
    fn collect_candidates(&self, out: &mut Vec<SectionCandidate>);
    fn unlinked_tasks(&self) -> Vec<SectionLinkTask>;
}

impl SectionLinkSource for Plan {
    fn collect_candidates(&self, out: &mut Vec<SectionCandidate>) {
        collect_sections(&self.sections, out);
    }

    fn unlinked_tasks(&self) -> Vec<SectionLinkTask> {
        self.tasks
            .iter()
            .filter(|task| task.section_ids.is_empty())
            .map(|task| SectionLinkTask {
                id: task.id.clone(),
                name: task.name.clone(),
                handle: task.handle.clone(),
                group: task.group.clone(),
                section_ids: task.section_ids.clone(),
            })
            .collect()
    }
}

impl SectionLinkSource for SectionLinkInput {
    fn collect_candidates(&self, out: &mut Vec<SectionCandidate>) {
        out.extend(self.sections.iter().map(|section| {
            candidate_fields(
                &section.id,
                &section.title,
                section.handle.as_deref(),
                &section.tags,
            )
        }));
    }

    fn unlinked_tasks(&self) -> Vec<SectionLinkTask> {
        self.tasks
            .iter()
            .filter(|task| task.section_ids.is_empty())
            .cloned()
            .collect()
    }
}
