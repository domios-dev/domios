use super::{PlanError, PlanStore};
use crate::plan::PlanStatus;

#[derive(Debug, serde::Deserialize)]
pub struct PlanDocumentMetadata {
    pub id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub status: PlanStatus,
}

#[derive(serde::Deserialize)]
pub(super) struct SectionMarkdownRead {
    pub title: String,
    pub body: String,
    pub depth: usize,
}

impl PlanStore {
    pub fn attachment_directory(&self, id: &str) -> Result<std::path::PathBuf, PlanError> {
        let metadata = self.database.document_metadata(id)?;
        Ok(self.plan_data_dir(&metadata.id))
    }

    pub fn document_metadata(&self, id: &str) -> Result<PlanDocumentMetadata, PlanError> {
        self.database.document_metadata(id)
    }

    pub fn spec_markdown(&self, id: &str) -> Result<String, PlanError> {
        let mut output = String::new();
        for section in self.database.section_markdown_rows(id)? {
            output.push_str(&format!(
                "{} {}\n\n",
                "#".repeat(section.depth.min(6)),
                section.title
            ));
            if !section.body.is_empty() {
                output.push_str(&section.body);
                output.push_str("\n\n");
            }
        }
        Ok(output)
    }

    pub fn loop_plan_id(&self, loop_id: &str) -> Result<Option<String>, PlanError> {
        self.database.loop_plan_id(loop_id)
    }
}
