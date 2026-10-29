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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<AgentTaskMedia>,
    pub timeout_ms: u64,
    pub max_tokens: u32,
    pub max_turns: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskMediaKind {
    Image,
    Audio,
    Video,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTaskMedia {
    pub kind: AgentTaskMediaKind,
    pub content_type: String,
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub lineage: crate::blueprint_step::InputLineage,
}

pub fn validate_media_capability(
    input: &AgentTaskInput,
    supported: &[AgentTaskMediaKind],
) -> Result<(), String> {
    if input
        .media
        .iter()
        .any(|media| !supported.contains(&media.kind))
    {
        return Err("agent backend does not support the supplied media modality".into());
    }
    Ok(())
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
    use sha2::{Digest, Sha256};
    if input.media.len() > 4
        || input
            .media
            .iter()
            .map(|media| media.bytes.len())
            .sum::<usize>()
            > 8 * 1024 * 1024
    {
        return Err("agent media exceeds bounded input".into());
    }
    for media in &input.media {
        let family = match media.kind {
            AgentTaskMediaKind::Image => "image/",
            AgentTaskMediaKind::Audio => "audio/",
            AgentTaskMediaKind::Video => "video/",
            AgentTaskMediaKind::File => "unsupported/",
        };
        let mime = media.content_type.starts_with(family)
            && serde_json::from_value::<crate::media_delivery::MediaType>(serde_json::json!(
                media.content_type
            ))
            .ok()
            .is_some_and(|mime| mime.matches(&media.bytes));
        if !mime
            || media.bytes.is_empty()
            || media.bytes.len() > 2 * 1024 * 1024
            || media.file_name.is_empty()
            || media.file_name.len() > 255
            || media.file_name.contains(['/', '\\'])
            || media.file_name.chars().any(char::is_control)
            || media.sha256 != format!("{:x}", Sha256::digest(&media.bytes))
        {
            return Err("invalid bounded agent media".into());
        }
    }
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
