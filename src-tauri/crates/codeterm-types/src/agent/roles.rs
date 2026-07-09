use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentRole {
    Orchestrator,
    Manager,
    Worker,
}

impl AgentRole {
    pub fn from_str_compat(s: &str) -> Option<Self> {
        match s {
            "Orchestrator" | "Conductor" => Some(Self::Orchestrator),
            "Manager" => Some(Self::Manager),
            "Worker" => Some(Self::Worker),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Done,
    InReview,
    Blocked,
    Partial,
}

impl ReportStatus {
    pub fn label(self) -> &'static str {
        match self {
            ReportStatus::Done => "Done",
            ReportStatus::InReview => "In Review",
            ReportStatus::Blocked => "Blocked",
            ReportStatus::Partial => "Partial",
        }
    }
}
