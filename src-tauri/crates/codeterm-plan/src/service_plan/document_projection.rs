use super::{PlanError, PlanStore};
use crate::plan::{
    Goal, OrchestratorRef, PlanStatus, PlannerInboxEntry, PlannerRef, PlannerRequest,
};
pub(super) mod cleanup;
mod coverage;
mod manifest;
mod regression;
mod tags;

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

#[derive(Debug, serde::Deserialize)]
pub struct PlannerGateRead {
    pub owner: OrchestratorRef,
    pub planner: Option<PlannerRef>,
    pub request: Option<PlannerRequest>,
    pub delivery: Option<PlannerInboxEntry>,
}

impl PlannerGateRead {
    pub fn owner_for_gate(&self, asked_by_tab_id: Option<&str>) -> Option<&str> {
        Self::owner_for_gate_fields(&self.owner, self.planner.as_ref(), asked_by_tab_id)
    }

    pub fn owner_for_gate_fields<'a>(
        owner: &'a OrchestratorRef,
        planner: Option<&PlannerRef>,
        asked_by_tab_id: Option<&str>,
    ) -> Option<&'a str> {
        let PlannerRef::Tab { tab_id, .. } = planner? else {
            return None;
        };
        (asked_by_tab_id == Some(tab_id.as_str()))
            .then_some(owner.tab_id.as_deref())
            .flatten()
    }
}

impl PlanStore {
    pub fn planner_gate_context(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<Option<PlannerGateRead>, PlanError> {
        self.database.planner_gate_context(id, request_id)
    }

    pub fn goal_document(&self, id: &str) -> Result<Option<Goal>, PlanError> {
        self.database.goal_document(id)
    }

    pub fn planner_nudge_projection(
        &self,
        id: &str,
        caller_tab_id: Option<&str>,
    ) -> Result<Option<String>, PlanError> {
        self.database
            .planner_nudge_input(id, caller_tab_id)
            .map(|input| {
                input.and_then(|input| super::hints::planner_ownership_nudge(input, caller_tab_id))
            })
    }

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

pub(super) mod team;
