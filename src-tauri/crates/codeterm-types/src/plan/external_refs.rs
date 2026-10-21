use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalRefKind {
    GithubIssue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalRefRelationship {
    Fixes,
    RelatesTo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalRef {
    pub kind: ExternalRefKind,
    pub repository: String,
    pub number: u64,
    pub canonical_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship: Option<ExternalRefRelationship>,
}
