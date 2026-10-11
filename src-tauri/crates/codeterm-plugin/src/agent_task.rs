use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::converter::ConversionCancellation;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTaskInput {
    pub run_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    pub profile: String,
    pub task: String,
    pub input: String,
    pub timeout_ms: u64,
    pub max_tokens: u32,
    pub max_turns: u16,
}

pub struct AgentTaskRequest {
    pub input: AgentTaskInput,
    pub deadline: Instant,
    pub cancellation: ConversionCancellation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskStatus {
    Done,
    Failed,
    Cancelled,
    DeadlineExceeded,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTaskResult {
    pub status: AgentTaskStatus,
    pub output: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn validate_input(input: &AgentTaskInput) -> Result<(), String> {
    if input.task.is_empty()
        || input.task.len() > 32768
        || input.input.len() > 32768
        || input.task.contains('\0')
        || input.input.contains('\0')
        || input.run_id.is_empty()
        || input.run_id.len() > 100
        || !input
            .run_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || input.tab_id.is_empty()
        || input.tab_id.len() > 128
        || input.workspace_id.is_empty()
        || input.workspace_id.len() > 128
        || input.profile.is_empty()
        || input.profile.len() > 128
        || input.max_tokens == 0
        || input.max_tokens > 1_000_000
        || input.max_turns == 0
        || input.max_turns > 100
        || input.timeout_ms == 0
        || input.timeout_ms > 3_600_000
    {
        return Err("invalid bounded agent task".into());
    }
    Ok(())
}

pub fn validate_result(result: AgentTaskResult) -> Result<AgentTaskResult, String> {
    if serde_json::to_vec(&result)
        .map_err(|e| e.to_string())?
        .len()
        > 65536
    {
        return Err("agent result exceeds 64 KiB".into());
    }
    if result.status == AgentTaskStatus::Done && result.error.is_some() {
        return Err("completed agent result cannot carry an error".into());
    }
    Ok(result)
}
