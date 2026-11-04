use crate::plan::TaskFiles;

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(in crate::service_plan) struct SharedDispatchRead {
    pub task_id: String,
    pub checkout: String,
    pub files: TaskFiles,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(in crate::service_plan) struct SharedReportRead {
    pub attempt_id: String,
    pub executor_tab_id: String,
    pub checkout: Option<String>,
    pub start_sha: Option<String>,
    pub files: TaskFiles,
}
