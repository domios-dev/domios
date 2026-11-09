use crate::service_plan::{body_files, PlanError, PlanStore};

impl PlanStore {
    pub fn file_manifest(&self, id: &str) -> Result<body_files::PlanFileManifest, PlanError> {
        let input = self.database.file_manifest_input(id)?;
        Ok(body_files::file_manifest(&input))
    }
}
