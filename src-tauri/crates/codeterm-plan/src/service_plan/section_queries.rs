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
        let section = self.database.selected_section(plan_id, section_id)?;
        Ok(section_details(plan_id.into(), section))
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

fn find_section<'a>(sections: &'a [Section], selector: &str) -> Option<&'a Section> {
    for section in sections {
        if section.id == selector || section.handle.as_deref() == Some(selector) {
            return Some(section);
        }
        if let Some(found) = find_section(&section.subsections, selector) {
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
mod tests;
