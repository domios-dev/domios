//! Per-pane JSONL chat tailers — UI-mode streaming pipeline.
//!
//! When a user flips a pane into UI mode the API caller starts a
//! `Tailer` keyed on `pane_id`. The tailer polls the session JSONL
//! through the provider plugin and emits delta messages via a
//! [`DeltaSink`]. The tailer keeps a byte-offset bookmark so each
//! poll only carries new content.
//!
//! All provider-specific logic (where the JSONL lives, how to parse
//! it) lives in plugins — this module only orchestrates
//! start/stop and shuttles the bytes through.

pub mod raw_text_sink;
pub mod registry;
pub mod structured_poller;
pub mod structured_sink;
pub mod tailer;
pub mod tauri_sink;

pub use registry::ChatRegistry;
pub use structured_poller::{spawn_structured_poller, StructuredPollConfig};
pub use tailer::{
    spawn_raw_tailer, spawn_tailer, DeltaSink, RawTailConfig, RawTextSink, TailConfig, TailerHandle,
};

#[cfg(test)]
mod tailer_tests;

use crate::agent::chat_types::{ChatMsg, RichMessage};

/// Flatten a JSONL RichMessage (role + typed blocks) into the same flat
/// ChatMsg shape opencode emits, so the read endpoint is provider-uniform.
pub fn rich_to_chat_msgs(rich: &RichMessage, base_id: usize) -> Vec<ChatMsg> {
    let mut out = Vec::new();
    for (i, b) in rich.blocks.iter().enumerate() {
        let id = format!("{base_id}-{i}");
        let get = |k: &str| b.data.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let msg = match b.kind.as_str() {
            "text" => ChatMsg { id, kind: rich.role.clone(), content: get("text"), ..Default::default() },
            "thinking" => ChatMsg { id, kind: "thinking".into(), content: get("text"), ..Default::default() },
            "tool_use" => ChatMsg {
                id,
                kind: "tool_call".into(),
                content: String::new(),
                tool_name: b.data.get("name").and_then(|v| v.as_str()).map(String::from),
                tool_input: b.data.get("input").cloned(),
                ..Default::default()
            },
            "tool_result" => ChatMsg {
                id,
                kind: "tool_result".into(),
                content: String::new(),
                tool_result: Some(get("content")),
                ..Default::default()
            },
            _ => continue,
        };
        out.push(msg);
    }
    out
}

/// Stateless normalized-chat read for a pane: parse the whole session
/// (JSONL or sqlite) into a flat ChatMsg list, return messages[since..].
/// No tailer spawned. Returns (messages, total_count).
pub fn pane_chat_snapshot(
    svc: &crate::service::ServiceContext,
    pane_id: &str,
    since: usize,
) -> Result<(Vec<ChatMsg>, usize), String> {
    let (provider_id, cwd, session_id) = {
        let m = svc.mux.lock();
        let pane = m.get_pane(pane_id).ok_or("pane not found")?;
        let provider = pane.agent_type.clone().ok_or("pane has no agent_type")?;
        let cwd = pane.cwd.clone().unwrap_or_default();
        let sid = pane.agent_session_id.clone().ok_or("pane has no session_id yet")?;
        (provider, cwd, sid)
    };

    let structured = svc
        .agent_registry
        .with_provider(&provider_id, |p| p.uses_structured_chat())
        .unwrap_or(false);

    let all: Vec<ChatMsg> = if structured {
        svc.agent_registry
            .with_provider(&provider_id, |p| p.read_structured_chat(&cwd, &session_id, None))
            .map(|sc| sc.messages)
            .unwrap_or_default()
    } else {
        let path = svc
            .agent_registry
            .with_provider(&provider_id, |p| p.session_file_path(&cwd, &session_id))
            .flatten()
            .ok_or("provider has no JSONL and no structured chat")?;
        let delta = svc
            .agent_registry
            .with_provider(&provider_id, |p| p.parse_session_delta(&path, 0))
            .ok_or("provider missing")?;
        delta
            .messages
            .iter()
            .enumerate()
            .flat_map(|(i, r)| rich_to_chat_msgs(r, i))
            .collect()
    };

    let total = all.len();
    let slice = all.into_iter().skip(since).collect();
    Ok((slice, total))
}

#[cfg(test)]
mod chat_snapshot_tests {
    use super::*;
    use crate::agent::chat_types::{MessageBlock, RichMessage};
    use serde_json::json;

    #[test]
    fn rich_text_and_tool_flatten() {
        let rich = RichMessage {
            role: "assistant".into(),
            ts: None,
            blocks: vec![
                MessageBlock { kind: "text".into(), data: json!({"text": "hi"}) },
                MessageBlock {
                    kind: "tool_use".into(),
                    data: json!({"name": "Bash", "input": {"command": "codeterm pane list"}}),
                },
            ],
        };
        let msgs = rich_to_chat_msgs(&rich, 0);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].kind, "assistant");
        assert_eq!(msgs[0].content, "hi");
        assert_eq!(msgs[1].kind, "tool_call");
        assert_eq!(msgs[1].tool_name.as_deref(), Some("Bash"));
        assert_eq!(msgs[1].tool_input.as_ref().unwrap()["command"], "codeterm pane list");
    }
}
