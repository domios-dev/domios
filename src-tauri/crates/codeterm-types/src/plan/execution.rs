use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskExecutionIntent {
    #[default]
    RepositoryMutation,
    NonRepository,
}

impl TaskExecutionIntent {
    pub fn requires_repository(self) -> bool {
        matches!(self, Self::RepositoryMutation)
    }
}
