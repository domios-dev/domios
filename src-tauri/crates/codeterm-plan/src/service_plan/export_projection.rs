use super::{PlanError, PlanStore};
use crate::plan::PlanStatus;
use serde_json::Value;

pub(super) mod header;

/// Encodes a complete requested record at the export boundary, never as a selective query.
pub trait PlanExportEncoder {
    fn encode<T: serde::Serialize>(&self, record: &T) -> Result<Value, PlanError>;
}

pub(super) struct JsonPlanExport;

impl PlanExportEncoder for JsonPlanExport {
    fn encode<T: serde::Serialize>(&self, record: &T) -> Result<Value, PlanError> {
        serde_json::to_value(record).map_err(|error| PlanError::Io(error.to_string()))
    }
}

impl PlanStore {
    pub fn spec_full(&self, id: &str) -> Result<Option<Value>, PlanError> {
        self.database.spec_full(id)
    }

    pub fn effort_bundle(&self, id: &str) -> Result<Option<Value>, PlanError> {
        self.database.effort_bundle(id)
    }

    pub fn export_plan_document(&self, id: &str) -> Result<Value, PlanError> {
        self.database.export_plan_document(id)
    }

    pub fn export_plan_documents(&self, ids: &[String]) -> Result<Vec<Value>, PlanError> {
        self.database.export_plan_documents(ids)
    }

    pub fn export_all_plan_documents(&self) -> Result<Vec<Value>, PlanError> {
        self.database.export_all_plan_documents()
    }

    pub fn export_all_plan_documents_with(
        &self,
        encoder: &impl PlanExportEncoder,
    ) -> Result<Vec<Value>, PlanError> {
        self.database.export_all_plan_documents_with(encoder)
    }

    pub fn export_plan_list(
        &self,
        project: Option<&str>,
        status: Option<PlanStatus>,
        unowned: bool,
        owner: Option<&str>,
    ) -> Result<Vec<Value>, PlanError> {
        self.database
            .export_plan_list(project, status, unowned, owner)
    }
}
