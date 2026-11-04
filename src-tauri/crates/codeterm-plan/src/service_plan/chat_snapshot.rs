use super::PlanStore;
use serde_json::Value;

pub(in crate::service_plan) mod read;

impl PlanStore {
    pub fn chat_snapshot_for_tab(
        &self,
        project: Option<&str>,
        team: Option<&str>,
        query: Option<&str>,
        tab: Option<&str>,
    ) -> Result<Vec<Value>, super::PlanError> {
        let project = project.map(|path| {
            super::paths::project_root(std::path::Path::new(path))
                .display()
                .to_string()
        });
        let query = query.map(str::to_lowercase);
        self.database
            .chat_projection(project.as_deref(), team, query.as_deref(), None, tab)
    }

    pub fn chat_snapshot(
        &self,
        project: Option<&str>,
        team: Option<&str>,
        query: Option<&str>,
    ) -> Result<Vec<Value>, super::PlanError> {
        self.chat_snapshot_for_tab(project, team, query, None)
    }
}

#[cfg(test)]
mod tests;
