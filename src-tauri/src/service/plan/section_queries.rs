use serde::Serialize;

use super::{PlanError, PlanStore};
use crate::plan::Section;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SectionDetails {
    pub container_id: String,
    pub section: Section,
    pub title: String,
    pub markdown: String,
}

impl PlanStore {
    pub fn show_section(
        &self,
        plan_id: &str,
        section_id: &str,
    ) -> Result<SectionDetails, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let section = find_section(&plan.sections, section_id)
            .cloned()
            .ok_or(PlanError::NotFound)?;
        Ok(section_details(plan.id, section))
    }

    pub fn show_spec_section(
        &self,
        spec_id: &str,
        section_id: &str,
    ) -> Result<SectionDetails, PlanError> {
        let spec = self.get_spec(spec_id).ok_or(PlanError::NotFound)?;
        let section = find_section(&spec.sections, section_id)
            .cloned()
            .ok_or(PlanError::NotFound)?;
        Ok(section_details(spec.id, section))
    }
}

fn section_details(container_id: String, section: Section) -> SectionDetails {
    let title = section.title.clone();
    let markdown = render_section_markdown(&section, 1);
    SectionDetails {
        container_id,
        title,
        markdown,
        section,
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

fn render_section_markdown(section: &Section, depth: usize) -> String {
    let mut out = format!("{} {}\n", "#".repeat(depth.min(6)), section.title);
    if !section.body.trim().is_empty() {
        out.push('\n');
        out.push_str(section.body.trim_end());
        out.push('\n');
    }
    for child in &section.subsections {
        out.push('\n');
        out.push_str(&render_section_markdown(child, depth + 1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};

    fn store_with_section() -> (tempfile::TempDir, PlanStore, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Sections".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_sections".into();
        store.save(&mut plan, 1).unwrap();
        let parent = store
            .add_section(&plan.id, "Parent", "Parent body", None, &[], None, None, 2)
            .unwrap();
        let child = store
            .add_section(
                &plan.id,
                "Child",
                "Child body",
                Some(&parent),
                &[],
                None,
                None,
                3,
            )
            .unwrap();
        (dir, store, plan.id, child)
    }

    #[test]
    fn show_section_finds_nested_section() {
        let (_dir, store, plan_id, child) = store_with_section();

        let details = store.show_section(&plan_id, &child).unwrap();

        assert_eq!(details.section.id, child);
        assert_eq!(details.title, "Child");
        assert!(details.markdown.contains("# Child"));
        assert!(details.markdown.contains("Child body"));
    }

    #[test]
    fn show_section_missing_is_not_found() {
        let (_dir, store, plan_id, _child) = store_with_section();

        assert!(matches!(
            store.show_section(&plan_id, "sec_missing"),
            Err(PlanError::NotFound)
        ));
    }
}
