//! JS↔native bridging for `JsProvider`: camelCase serde mirrors of the
//! provider wire types, JSON conversion helpers, and the VM-backed usage
//! parser adapter. No provider logic lives here — pure shape translation.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use super::super::{call_json, PluginVm};
use crate::provider::{AgentLaunchParams, SessionMessage, SessionMetadata};

/// CamelCase mirror of `SessionMetadata` so plugin authors can return
/// `{ sessionId, projectPath, ... }` instead of snake_case.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SerdeSessionMetadata {
    session_id: String,
    provider: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    file_path: String,
    #[serde(default)]
    transcript_path: Option<String>,
    #[serde(default)]
    created_ts: Option<i64>,
    #[serde(default)]
    modified_ts: Option<i64>,
}

const SESSION_HEAD_BYTES: u64 = 64 * 1024;

/// Bytes handed to `parseSessionMessages` per QuickJS call. Chunks are cut on
/// line boundaries, so a JSONL parser sees the same token stream it would get
/// from the whole file — without either side holding a multi-gigabyte string.
pub(super) const SESSION_PARSE_CHUNK_BYTES: usize = 4 * 1024 * 1024;

pub(super) fn session_description_head(path: &Path) -> String {
    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    // Bound the reader itself: a session whose first line is gigabytes long
    // would otherwise be materialised before the per-line budget is consulted.
    let mut head = String::new();
    for line in BufReader::new(std::io::Read::take(file, SESSION_HEAD_BYTES))
        .lines()
        .map_while(Result::ok)
        .take(64)
    {
        head.push_str(&line);
        head.push('\n');
    }
    head
}

impl SerdeSessionMetadata {
    pub(super) fn into_session_metadata(self, fallback_provider: &str) -> SessionMetadata {
        SessionMetadata {
            session_id: self.session_id,
            provider: self
                .provider
                .unwrap_or_else(|| fallback_provider.to_string()),
            project_path: self.project_path,
            title: self.title,
            summary: self.summary,
            file_path: self.file_path,
            transcript_path: self.transcript_path,
            created_ts: self.created_ts,
            modified_ts: self.modified_ts,
        }
    }
}

/// CamelCase mirror of `SessionMessage` so plugin authors don't have to
/// remember `is_meta` over `isMeta`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SerdeSessionMessage {
    role: String,
    text: String,
    #[serde(default)]
    ts: Option<i64>,
    #[serde(default)]
    is_meta: bool,
}

impl From<SerdeSessionMessage> for SessionMessage {
    fn from(s: SerdeSessionMessage) -> Self {
        SessionMessage {
            role: s.role,
            text: s.text,
            ts: s.ts,
            is_meta: s.is_meta,
        }
    }
}

/// Bridges the `UsageParser` trait to a plugin's `parseUsage(rawText, nowMs)`
/// export. The plugin returns a `UsageSnapshot`-shaped object (snake_case
/// fields) or null when nothing was recognized.
pub(super) struct VmUsageParser {
    pub(super) vm: Arc<PluginVm>,
    pub(super) plugin_id: String,
}

impl crate::usage_parser::UsageParser for VmUsageParser {
    fn parse(
        &self,
        raw_text: &str,
    ) -> Result<crate::usage_parser::UsageSnapshot, crate::usage_parser::UsageParseError> {
        use crate::usage_parser::UsageParseError;
        let now_ms = chrono::Utc::now().timestamp_millis();
        match call_json::<Option<crate::usage_parser::UsageSnapshot>>(
            &self.vm,
            "parseUsage",
            &[json!(raw_text), json!(now_ms)],
        ) {
            Ok(Some(snap)) => Ok(snap),
            Ok(None) => Err(UsageParseError::NothingFound),
            Err(e) => Err(UsageParseError::Other(format!(
                "[plugin {}] parseUsage failed: {e}",
                self.plugin_id
            ))),
        }
    }
}

pub(super) fn encode_cwd_dashed(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

pub(super) fn launch_params_to_json(p: &AgentLaunchParams) -> Value {
    json!({
        "agentName": p.agent_name,
        "task": p.task,
        "starterPrompt": p.starter_prompt,
        "starterPromptText": p.starter_prompt.host_text(),
        "starterPromptPrefix": p.starter_prompt.system_prefix(),
        "args": p.args,
        "skipPermissions": p.skip_permissions,
        "sessionId": p.session_id,
        "structuredPromptServer": p.structured_prompt_server,
        "launchMarker": p.launch_marker,
    })
}

pub(super) fn result_from_js(v: &serde_json::Value) -> Result<(), String> {
    let ok = v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false);
    if ok {
        return Ok(());
    }
    let err = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("plugin returned ok=false");
    Err(err.to_string())
}
