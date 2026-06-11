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

use crate::agent::chat_types::{ChatAttachment, ChatMsg, RichMessage};

const THINKING_PREVIEW: usize = 200;

/// Short, human-readable summary of a tool input (file path, command, …),
/// mirroring the desktop parser's `summarizeInput`.
fn summarize_tool_input(input: Option<&serde_json::Value>) -> String {
    let Some(obj) = input.and_then(|v| v.as_object()) else {
        return String::new();
    };
    let s = |k: &str| obj.get(k).and_then(|v| v.as_str());
    let take = |v: &str, n: usize| v.chars().take(n).collect::<String>();
    if let Some(v) = s("file_path") {
        return v.to_string();
    }
    if let Some(v) = s("command") {
        return take(v, 80);
    }
    if let Some(v) = s("pattern") {
        return v.to_string();
    }
    if let Some(v) = s("prompt") {
        return format!("{}...", take(v, 60));
    }
    if let Some(v) = s("description") {
        return take(v, 60);
    }
    take(&serde_json::to_string(obj).unwrap_or_default(), 60)
}

/// A tool_result block's `content` is a string, an array of `{text}` blocks,
/// or an arbitrary object — normalize all three to plain text.
fn extract_tool_result_text(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .map(|c| c.get("text").and_then(|v| v.as_str()).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

/// Detect a single wrapping local-command XML tag, mirroring the desktop
/// `extractSingleXmlTag`. Returns (tag, inner body) on a full-string match.
fn extract_local_command(content: &str) -> Option<(String, String)> {
    let trimmed = content.trim();
    const TAGS: [&str; 5] = [
        "local-command-caveat",
        "local-command-stdout",
        "command-name",
        "command-message",
        "command-args",
    ];
    for tag in TAGS {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        if trimmed.starts_with(&open) && trimmed.ends_with(&close) {
            let body = &trimmed[open.len()..trimmed.len() - close.len()];
            return Some((tag.to_string(), body.to_string()));
        }
    }
    None
}

/// System provenance for an `isMeta` row, mirroring the desktop `metaSubtype`.
fn meta_subtype(content: &str) -> String {
    if content.trim_start().starts_with("Base directory for this skill:") {
        "skill_context".to_string()
    } else {
        "meta".to_string()
    }
}

fn strip_ansi(s: &str) -> String {
    // Minimal CSI/SGR stripper for local-command stdout bodies.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Build an image attachment from a JSONL image block's `source`.
fn attachment_from_image(data: &serde_json::Value, id: String) -> ChatAttachment {
    let source = data.get("source");
    let mime_type = source
        .and_then(|s| s.get("media_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("image/png")
        .to_string();
    let b64 = source.and_then(|s| s.get("data")).and_then(|v| v.as_str());
    ChatAttachment {
        id,
        mime_type,
        // Drop oversized inline payloads (mirror the desktop 100KB cap).
        data: b64.filter(|d| d.len() < 100_000).map(String::from),
        url: source
            .and_then(|s| s.get("url"))
            .and_then(|v| v.as_str())
            .map(String::from),
        path: source
            .and_then(|s| s.get("path"))
            .and_then(|v| v.as_str())
            .map(String::from),
    }
}

/// Flatten a JSONL RichMessage (role + typed blocks) into the normalized
/// `ChatMsg` shape — the single canonical chat shape shared with opencode's
/// structured path and the frontend `NormalizedChatMessage`. This is the one
/// place JSONL providers' rich rendering lives; the desktop renders the same
/// output, so there is no second parser.
pub fn rich_to_chat_msgs(rich: &RichMessage, base_id: usize) -> Vec<ChatMsg> {
    let mut out = Vec::new();
    let role = rich.role.as_str();
    let base = rich
        .uuid
        .clone()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| base_id.to_string());
    let id_at = |i: usize| format!("{base}-{i}");
    let ts = rich.timestamp.clone();

    if role == "system" {
        let content = rich
            .blocks
            .first()
            .and_then(|b| b.data.get("text"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let subtype = rich
            .blocks
            .first()
            .and_then(|b| b.data.get("subtype"))
            .and_then(|v| v.as_str())
            .map(String::from);
        if let Some((tag, body)) = extract_local_command(&content) {
            let kind = if tag == "local-command-caveat" { "system" } else { "local_command" };
            out.push(ChatMsg {
                id: id_at(0),
                kind: kind.into(),
                content: strip_ansi(&body).trim().to_string(),
                tag_name: Some(tag),
                system_subtype: subtype,
                timestamp: ts,
                ..Default::default()
            });
        } else if !content.trim().is_empty() {
            out.push(ChatMsg {
                id: id_at(0),
                kind: "system".into(),
                content: content.trim().to_string(),
                system_subtype: subtype,
                timestamp: ts,
                ..Default::default()
            });
        }
        return out;
    }

    if role == "user" {
        let mut text_parts: Vec<String> = Vec::new();
        let mut attachments: Vec<ChatAttachment> = Vec::new();
        let mut emitted_tool_result = false;
        for b in rich.blocks.iter() {
            match b.kind.as_str() {
                "text" => {
                    let t = b.data.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    if !t.trim().is_empty() {
                        text_parts.push(t.trim().to_string());
                    }
                }
                "image" => {
                    let aid = format!("{base}-img-{}", attachments.len());
                    attachments.push(attachment_from_image(&b.data, aid));
                }
                "tool_result" => {
                    // Real Claude pattern: bash/grep/read output arrives as a
                    // tool_result block in the user turn after the tool_use.
                    emitted_tool_result = true;
                    out.push(ChatMsg {
                        id: format!("result-{}", b
                            .data
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&base)),
                        kind: "tool_result".into(),
                        tool_id: b
                            .data
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        tool_result: Some(extract_tool_result_text(b.data.get("content"))),
                        collapsed: true,
                        timestamp: ts.clone(),
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
        let content = text_parts.join("\n");
        // Pure-text user turns may be a local-command echo or an isMeta system
        // row — mirror the desktop's string-content handling.
        if attachments.is_empty() && !emitted_tool_result && !content.is_empty() {
            if let Some((tag, body)) = extract_local_command(&content) {
                let kind = if tag == "local-command-caveat" { "system" } else { "local_command" };
                out.push(ChatMsg {
                    id: id_at(0),
                    kind: kind.into(),
                    content: strip_ansi(&body).trim().to_string(),
                    tag_name: Some(tag),
                    timestamp: ts,
                    ..Default::default()
                });
                return out;
            }
            if rich.is_meta == Some(true) {
                out.push(ChatMsg {
                    id: id_at(0),
                    kind: "system".into(),
                    content: content.clone(),
                    system_subtype: Some(meta_subtype(&content)),
                    timestamp: ts,
                    ..Default::default()
                });
                return out;
            }
        }
        if !content.is_empty() || !attachments.is_empty() {
            let n = attachments.len();
            out.push(ChatMsg {
                id: id_at(0),
                kind: "user".into(),
                content: if !content.is_empty() {
                    content
                } else {
                    format!("[{} image{}]", n, if n > 1 { "s" } else { "" })
                },
                attachments: if attachments.is_empty() {
                    None
                } else {
                    Some(attachments)
                },
                timestamp: ts,
                ..Default::default()
            });
        }
        return out;
    }

    for (i, b) in rich.blocks.iter().enumerate() {
        let id = id_at(i);
        let get = |k: &str| b.data.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut msg = match b.kind.as_str() {
            "text" => {
                let t = get("text");
                if t.trim().is_empty() {
                    continue;
                }
                ChatMsg { id, kind: role.to_string(), content: t.trim().to_string(), ..Default::default() }
            }
            "thinking" => {
                // Real claude thinking blocks carry `thinking`; some carry `text`.
                let raw = b
                    .data
                    .get("thinking")
                    .or_else(|| b.data.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if raw.trim().is_empty() {
                    continue;
                }
                let truncated: String = raw.chars().take(THINKING_PREVIEW).collect();
                let content = if raw.chars().count() > THINKING_PREVIEW {
                    format!("{truncated}...")
                } else {
                    truncated
                };
                ChatMsg { id, kind: "thinking".into(), content, collapsed: true, ..Default::default() }
            }
            "tool_use" => {
                let name = get("name");
                let args = summarize_tool_input(b.data.get("input"));
                ChatMsg {
                    id,
                    kind: "tool_call".into(),
                    content: format!("{name}({args})"),
                    tool_name: Some(name),
                    tool_args: Some(args),
                    tool_input: b.data.get("input").cloned(),
                    tool_id: b.data.get("id").and_then(|v| v.as_str()).map(String::from),
                    collapsed: true,
                    ..Default::default()
                }
            }
            "tool_result" => ChatMsg {
                id,
                kind: "tool_result".into(),
                tool_id: b.data.get("tool_use_id").and_then(|v| v.as_str()).map(String::from),
                tool_result: Some(extract_tool_result_text(b.data.get("content"))),
                collapsed: true,
                ..Default::default()
            },
            "image" => {
                let source = b.data.get("source");
                ChatMsg {
                    id,
                    kind: "image".into(),
                    image_data: source.and_then(|s| s.get("data")).and_then(|v| v.as_str()).map(String::from),
                    image_mime_type: Some(
                        source
                            .and_then(|s| s.get("media_type"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("image/png")
                            .to_string(),
                    ),
                    ..Default::default()
                }
            }
            _ => continue,
        };
        msg.timestamp = ts.clone();
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
            blocks: vec![
                MessageBlock { kind: "text".into(), data: json!({"text": "hi"}) },
                MessageBlock {
                    kind: "tool_use".into(),
                    data: json!({"name": "Bash", "input": {"command": "codeterm pane list"}}),
                },
            ],
            ..Default::default()
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
