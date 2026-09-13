//! Per-tab JSONL chat tailers — UI-mode streaming pipeline.
//!
//! When a user flips a tab into UI mode the API caller starts a
//! `Tailer` keyed on `tab_id`. The tailer polls the session JSONL
//! through the provider plugin and emits delta messages via a
//! [`DeltaSink`]. The tailer keeps a byte-offset bookmark so each
//! poll only carries new content.
//!
//! All provider-specific logic (where the JSONL lives, how to parse
//! it) lives in plugins — this module only orchestrates
//! start/stop and shuttles the bytes through.

pub mod chatbackend_history;
pub mod context_window;
pub mod host_context;
pub mod raw_text_sink;
pub mod registry;
pub mod session_window;
pub mod store_watch;
pub mod structured_poller;
pub mod structured_sink;
pub mod tailer;
pub mod tauri_sink;

pub use registry::ChatRegistry;
pub use store_watch::{spawn_store_watch, StoreWatchConfig};
pub use structured_poller::{spawn_structured_poller, StructuredPollConfig};
pub use tailer::{
    spawn_raw_tailer, spawn_tailer, DeltaSink, RawTailConfig, RawTextSink, TailConfig, TailerHandle,
};

#[cfg(test)]
mod tailer_tests;

use crate::agent::chat_types::{
    ChatAttachment, ChatMsg, ChatTimelineMarker, InteractiveElement, RichMessage,
};

const THINKING_PREVIEW: usize = 200;
pub const LIVE_CHAT_POLL_INTERVAL_MS: u64 = 100;

pub fn live_chat_poll_interval() -> std::time::Duration {
    std::time::Duration::from_millis(LIVE_CHAT_POLL_INTERVAL_MS)
}

/// Populate `msg.interactive` from the provider's capability. Called at every
/// provider-in-scope tail/snapshot/backfill site so both wire shapes carry it.
pub fn enrich_interactive(
    msg: &mut RichMessage,
    provider: &dyn crate::agent::provider::AgentProvider,
) {
    msg.interactive = provider.parse_interactive_events(msg);
}

fn get_optional_string(data: &serde_json::Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(|value| value.as_str())
        .map(String::from)
}

/// A tool_result block's `content` is a string, an array of `{text}` blocks,
/// or an arbitrary object — normalize all three to plain text.
fn extract_tool_result_text(content: Option<&serde_json::Value>) -> String {
    let text = match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .map(|c| c.get("text").and_then(|v| v.as_str()).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    strip_ansi(&text)
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
    if content
        .trim_start()
        .starts_with("Base directory for this skill:")
    {
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

/// Byte origin of one parsed row: `offset` is the complete-record boundary the
/// batch started at, `ordinal` the row position inside that batch.
#[derive(Clone, Copy, Debug)]
pub struct RowOrigin {
    pub offset: u64,
    pub ordinal: usize,
}

impl RowOrigin {
    pub fn at(offset: u64, ordinal: usize) -> Self {
        Self { offset, ordinal }
    }

    fn id_base(&self) -> String {
        format!("{}-{}", self.offset, self.ordinal)
    }
}

/// Flatten a row parsed from a session byte range: ids anchor to the batch
/// record boundary, `seq` stays an ordinal inside the batch.
pub fn rich_to_chat_msgs_at(rich: &RichMessage, origin: RowOrigin) -> Vec<ChatMsg> {
    let base = origin.id_base();
    let mut out = apply_capabilities(rich, rich_to_chat_msgs_blocks(rich, &base), &base);
    for m in &mut out {
        m.seq = origin.ordinal as i64;
    }
    out
}

/// Unanchored flatten for callers with no session byte range; ids and `seq`
/// mean nothing outside that batch (send-evidence content extraction only).
pub fn rich_to_chat_msgs(rich: &RichMessage, base_id: usize) -> Vec<ChatMsg> {
    let base = base_id.to_string();
    let mut out = apply_capabilities(rich, rich_to_chat_msgs_blocks(rich, &base), &base);
    for m in &mut out {
        m.seq = base_id as i64;
    }
    out
}

fn apply_capabilities(
    rich: &RichMessage,
    mut messages: Vec<ChatMsg>,
    fallback_base: &str,
) -> Vec<ChatMsg> {
    let base = rich
        .uuid
        .clone()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| fallback_base.to_string());
    if messages.is_empty() && (rich.timeline_marker.is_some() || rich.session_intent.is_some()) {
        let (kind, content) = match rich.timeline_marker.as_ref() {
            Some(ChatTimelineMarker::ExternalEdit { path }) => {
                ("system", format!("changed outside: {path}"))
            }
            Some(ChatTimelineMarker::ContextCompaction) => {
                ("system", "context compacted".to_string())
            }
            Some(ChatTimelineMarker::Queued) | None => ("custom", String::new()),
        };
        messages.push(ChatMsg {
            id: format!("{base}-0"),
            kind: kind.into(),
            content,
            timestamp: rich.timestamp.clone(),
            ..Default::default()
        });
    }
    for message in &mut messages {
        message.timeline_marker = rich.timeline_marker.clone();
        message.session_intent = rich.session_intent.clone();
        match rich.timeline_marker.as_ref() {
            Some(ChatTimelineMarker::ExternalEdit { path }) => {
                message.kind = "system".into();
                message.content = format!("changed outside: {path}");
            }
            Some(ChatTimelineMarker::ContextCompaction) => {
                message.kind = "system".into();
                message.content = "context compacted".into();
            }
            Some(ChatTimelineMarker::Queued) | None => {}
        }
    }
    messages
}

fn rich_to_chat_msgs_blocks(rich: &RichMessage, fallback_base: &str) -> Vec<ChatMsg> {
    let mut out = Vec::new();
    let role = rich.role.as_str();
    let base = rich
        .uuid
        .clone()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| fallback_base.to_string());
    let id_at = |i: usize| format!("{base}-{i}");
    let ts = rich.timestamp.clone();
    let source_subtype = rich.source_subtype.as_deref();

    if rich.is_compact_summary == Some(true) {
        let content = rich
            .blocks
            .iter()
            .filter(|b| b.kind == "text")
            .filter_map(|b| b.data.get("text").and_then(|v| v.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !content.is_empty() {
            out.push(ChatMsg {
                id: id_at(0),
                kind: "system".into(),
                content,
                system_subtype: Some("compact_summary".into()),
                collapsed: true,
                timestamp: ts,
                ..Default::default()
            });
        }
        return out;
    }

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
            .or(source_subtype)
            .map(String::from);
        if matches!(
            subtype.as_deref(),
            Some("turn_duration" | "stop_hook_summary")
        ) {
            return out;
        }
        if subtype.as_deref() == Some("compact_boundary") {
            out.push(ChatMsg {
                id: id_at(0),
                kind: "status".into(),
                content: "Context compacted".into(),
                system_subtype: subtype,
                timestamp: ts,
                ..Default::default()
            });
            return out;
        }
        if let Some((tag, body)) = extract_local_command(&content) {
            let kind = if tag == "local-command-caveat" {
                "system"
            } else {
                "local_command"
            };
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
                    // tool_result block in the user turn after the tool_use.
                    emitted_tool_result = true;
                    let is_error = b.data.get("is_error").and_then(|v| v.as_bool());
                    out.push(ChatMsg {
                        id: format!(
                            "result-{}",
                            b.data
                                .get("tool_use_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&base)
                        ),
                        kind: "tool_result".into(),
                        tool_id: b
                            .data
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        tool_result: Some(extract_tool_result_text(b.data.get("content"))),
                        tool_error: is_error,
                        collapsed: true,
                        timestamp: ts.clone(),
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
        let content = text_parts.join("\n");
        // row — mirror the desktop's string-content handling.
        if attachments.is_empty() && !emitted_tool_result && !content.is_empty() {
            if rich.is_visible_in_transcript_only == Some(true) {
                out.push(ChatMsg {
                    id: id_at(0),
                    kind: "system".into(),
                    content: content.clone(),
                    system_subtype: Some("compact_summary".into()),
                    collapsed: true,
                    timestamp: ts,
                    ..Default::default()
                });
                return out;
            }
            if let Some((tag, body)) = extract_local_command(&content) {
                let kind = if tag == "local-command-caveat" {
                    "system"
                } else {
                    "local_command"
                };
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
        let get = |k: &str| {
            b.data
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let mut msg = match b.kind.as_str() {
            "text" => {
                let t = get("text");
                if t.trim().is_empty() {
                    continue;
                }
                ChatMsg {
                    id,
                    kind: role.to_string(),
                    content: t.trim().to_string(),
                    system_subtype: b
                        .data
                        .get("subtype")
                        .and_then(|v| v.as_str())
                        .or(source_subtype)
                        .map(String::from),
                    ..Default::default()
                }
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
                ChatMsg {
                    id,
                    kind: "thinking".into(),
                    content,
                    collapsed: true,
                    ..Default::default()
                }
            }
            "tool_use" => {
                let name = get("name");
                let tool_kind = get_optional_string(&b.data, "tool_kind");
                let tool_summary = get_optional_string(&b.data, "tool_summary");
                let tool_detail = get_optional_string(&b.data, "tool_detail");
                let tool_edits = b.data.get("tool_edits").cloned();
                let tool_description = get_optional_string(&b.data, "tool_description");
                let tool_title = get_optional_string(&b.data, "tool_title");
                let tool_flow_id = get_optional_string(&b.data, "tool_flow_id");
                let tool_echo = b.data.get("tool_echo").and_then(|v| v.as_bool());
                let tool_error = match b.data.get("tool_status").and_then(|v| v.as_str()) {
                    Some("success") => Some(false),
                    Some("error") => Some(true),
                    _ => None,
                };
                let tool_id = b.data.get("id").and_then(|v| v.as_str());
                let interactive = rich
                    .interactive
                    .iter()
                    .filter(|e| {
                        matches!(e, InteractiveElement::Question(q)
                            if Some(q.tool_use_id.as_str()) == tool_id)
                    })
                    .cloned()
                    .collect();
                ChatMsg {
                    id,
                    kind: "tool_call".into(),
                    content: tool_summary.clone().unwrap_or_default(),
                    tool_kind,
                    tool_summary,
                    tool_detail: tool_detail.clone(),
                    tool_edits,
                    tool_description,
                    tool_title,
                    tool_flow_id,
                    tool_echo,
                    tool_name: Some(name),
                    tool_args: tool_detail,
                    tool_input: b.data.get("input").cloned(),
                    tool_id: tool_id.map(String::from),
                    tool_error,
                    collapsed: true,
                    interactive,
                    ..Default::default()
                }
            }
            "tool_result" => {
                let is_error = b.data.get("is_error").and_then(|v| v.as_bool());
                ChatMsg {
                    id,
                    kind: "tool_result".into(),
                    tool_id: b
                        .data
                        .get("tool_use_id")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    tool_result: Some(extract_tool_result_text(b.data.get("content"))),
                    tool_error: is_error,
                    collapsed: true,
                    ..Default::default()
                }
            }
            "tool_flow" => {
                let state = get("state");
                if state != "start" && state != "end" {
                    continue;
                }
                ChatMsg {
                    id,
                    kind: "tool_flow".into(),
                    content: state,
                    tool_flow_id: get_optional_string(&b.data, "flow_id"),
                    ..Default::default()
                }
            }
            "image" => {
                let source = b.data.get("source");
                ChatMsg {
                    id,
                    kind: "image".into(),
                    image_data: source
                        .and_then(|s| s.get("data"))
                        .and_then(|v| v.as_str())
                        .map(String::from),
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

/// Stateless normalized-chat read for a tab: parse the whole session
/// (JSONL or sqlite) into a flat ChatMsg list, return messages[since..].
/// No tailer spawned. Returns (messages, total_count).
pub fn tab_chat_snapshot(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    since: usize,
) -> Result<(Vec<ChatMsg>, usize), String> {
    let backing = {
        let m = svc.mux.write();
        m.get_tab(tab_id).ok_or("Tab not found")?.backing.clone()
    };
    match backing {
        crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id,
            session_id,
        } => {
            let provider = svc
                .chatbackend_registry
                .get(&plugin_id)
                .ok_or_else(|| format!("chatBackend plugin '{plugin_id}' not registered"))?;
            chatbackend_transcript_snapshot(provider.as_ref(), &session_id, since)
        }
        crate::multiplexer::types::TabBacking::Pty => {
            let (provider_id, cwd, session_id) = {
                let m = svc.mux.write();
                let tab = m.get_tab(tab_id).ok_or("Tab not found")?;
                let provider = tab.agent_type.clone().ok_or("Tab has no agent_type")?;
                let cwd = tab.cwd.clone().unwrap_or_default();
                let sid = tab
                    .agent_session_id
                    .clone()
                    .ok_or("Tab has no session_id yet")?;
                (provider, cwd, sid)
            };
            session_chat_snapshot(svc, &provider_id, &cwd, &session_id, since)
        }
    }
}

/// Snapshot a chatBackend tab's transcript. The host-side JSONL is the DURABLE
/// truth: a chatBackend shell has no provider-side store, so the live plugin Map
/// is wiped on refresh / VM reload / daemon restart. We read the persisted JSONL
/// first ([`chatbackend_history`]) and only fall back to a live `poll` when no
/// JSONL exists yet (a brand-new session before the first delta was persisted).
/// Returns (messages[since..], total_count).
pub(crate) fn chatbackend_transcript_snapshot(
    provider: &dyn crate::chatbackend::ChatBackendProvider,
    session_id: &str,
    since: usize,
) -> Result<(Vec<ChatMsg>, usize), String> {
    let path = chatbackend_history::session_path(provider.id(), session_id);
    chatbackend_snapshot_from(provider, session_id, since, &path)
}

/// Path-injectable core of [`chatbackend_transcript_snapshot`] (hermetic tests).
fn chatbackend_snapshot_from(
    provider: &dyn crate::chatbackend::ChatBackendProvider,
    session_id: &str,
    since: usize,
    history_path: &std::path::Path,
) -> Result<(Vec<ChatMsg>, usize), String> {
    let from_disk = chatbackend_history::read_messages(history_path);
    let all = if from_disk.is_empty() {
        // cumulative transcript), exactly like the agent path.
        provider
            .poll(session_id, None)
            .map_err(|e| e.to_string())?
            .messages
    } else {
        from_disk
    };
    let total = all.len();
    let slice = all.into_iter().skip(since).collect();
    Ok((slice, total))
}

/// Tab-less twin of [`tab_chat_snapshot`]: parse a session's transcript
/// straight from `(provider, cwd, session_id)`. Used by the read-only chat-ui
/// to render closed hidden-worker / cron runs whose tab is already gone.
pub fn session_chat_snapshot(
    svc: &crate::service::ServiceContext,
    provider_id: &str,
    cwd: &str,
    session_id: &str,
    since: usize,
) -> Result<(Vec<ChatMsg>, usize), String> {
    let structured = svc
        .agent_registry
        .with_provider(provider_id, |p| p.uses_structured_chat())
        .unwrap_or(false);

    let all: Vec<ChatMsg> = if structured {
        svc.agent_registry
            .with_provider(provider_id, |p| {
                p.read_structured_chat(cwd, session_id, None)
            })
            .map(|sc| sc.messages)
            .unwrap_or_default()
    } else {
        let path = svc
            .agent_registry
            .with_provider(provider_id, |p| p.session_file_path(cwd, session_id))
            .flatten()
            .ok_or("provider has no JSONL and no structured chat")?;
        let snap = session_window::snapshot(&svc.agent_registry, provider_id, &path);
        session_window::log_bounded_snapshot(&format!("session:{session_id}"), provider_id, &snap);
        snap.messages
    };

    let total = all.len();
    let slice = all.into_iter().skip(since).collect();
    Ok((slice, total))
}

#[cfg(test)]
mod chat_snapshot_tests {
    use super::*;
    use crate::agent::chat_types::{MessageBlock, RichMessage};
    use crate::agent::provider::ModelInfo;
    use crate::chatbackend::{ChatBackendError, ChatBackendProvider};
    use serde_json::json;

    /// Cumulative-transcript backend (like the lmstudio plugin): `poll` with no
    /// cursor returns the FULL transcript, so the snapshot can slice it.
    struct SnapshotBackend {
        messages: Vec<ChatMsg>,
    }

    impl SnapshotBackend {
        fn with_messages(n: usize) -> Self {
            let messages = (0..n)
                .map(|i| ChatMsg {
                    id: i.to_string(),
                    kind: "assistant".into(),
                    content: format!("m{i}"),
                    ..Default::default()
                })
                .collect();
            Self { messages }
        }
    }

    impl ChatBackendProvider for SnapshotBackend {
        fn id(&self) -> &str {
            "snapshot"
        }
        fn open_session(&self, _ctx: serde_json::Value) -> Result<String, ChatBackendError> {
            Ok("sid".into())
        }
        fn send_message(&self, _session_id: &str, _text: &str) -> Result<(), ChatBackendError> {
            Ok(())
        }
        fn poll(
            &self,
            _session_id: &str,
            _cursor: Option<&str>,
        ) -> Result<crate::agent::chat_types::StructuredChat, ChatBackendError> {
            Ok(crate::agent::chat_types::StructuredChat {
                messages: self.messages.clone(),
                cursor: None,
                done: None,
            })
        }
        fn close_session(&self, _session_id: &str) -> Result<(), ChatBackendError> {
            Ok(())
        }
        fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn chatbackend_snapshot_returns_full_transcript() {
        let backend = SnapshotBackend::with_messages(3);
        let (msgs, total) = chatbackend_transcript_snapshot(&backend, "sid", 0).unwrap();
        assert_eq!(total, 3);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].content, "m0");
        assert_eq!(msgs[2].content, "m2");
    }

    #[test]
    fn chatbackend_snapshot_honors_since_cursor() {
        let backend = SnapshotBackend::with_messages(3);
        let (msgs, total) = chatbackend_transcript_snapshot(&backend, "sid", 2).unwrap();
        // total is the full count; the slice starts at `since`.
        assert_eq!(total, 3);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "m2");
    }

    #[test]
    fn chatbackend_snapshot_since_past_end_is_empty_not_error() {
        let backend = SnapshotBackend::with_messages(2);
        let (msgs, total) = chatbackend_transcript_snapshot(&backend, "sid", 5).unwrap();
        assert_eq!(total, 2);
        assert!(msgs.is_empty());
    }

    #[test]
    fn chatbackend_snapshot_reads_hand_written_jsonl() {
        // live `poll`, the persisted file wins (refresh shows real history).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(
            &path,
            "{\"id\":\"u1\",\"type\":\"user\",\"content\":\"hi\"}\n\
             {\"id\":\"a1\",\"type\":\"assistant\",\"content\":\"hello there\"}\n",
        )
        .unwrap();

        let backend = SnapshotBackend::with_messages(0); // live Map empty
        let (msgs, total) = super::chatbackend_snapshot_from(&backend, "sid", 0, &path).unwrap();
        assert_eq!(total, 2);
        assert_eq!(msgs[0].content, "hi");
        assert_eq!(msgs[1].content, "hello there");
    }

    #[test]
    fn restored_tab_replays_history_from_jsonl_when_live_map_wiped() {
        // (poll → 0 messages) but the JSONL persisted the full transcript.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut h = chatbackend_history::ChatBackendHistory::open_at(path.clone());
        h.upsert(&[
            ChatMsg {
                id: "1".into(),
                kind: "user".into(),
                content: "remember me".into(),
                ..Default::default()
            },
            ChatMsg {
                id: "2".into(),
                kind: "assistant".into(),
                content: "i do".into(),
                ..Default::default()
            },
        ]);

        let wiped = SnapshotBackend::with_messages(0);
        let (msgs, total) = super::chatbackend_snapshot_from(&wiped, "sid", 0, &path).unwrap();
        assert_eq!(total, 2, "history replays from disk, not the wiped Map");
        assert_eq!(msgs[0].content, "remember me");

        // `since` is honored on the disk-backed slice too.
        let (tail, _) = super::chatbackend_snapshot_from(&wiped, "sid", 1, &path).unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].content, "i do");
    }

    #[test]
    fn snapshot_after_poll_flush_includes_host_inbound_and_reply() {
        let plugin = "test-snapshot-single-writer-unique";
        let session = "snap-sw-sid";
        let path = chatbackend_history::session_path(plugin, session);
        let _ = std::fs::remove_file(&path);

        let shared = chatbackend_history::ChatBackendHistory::shared(plugin, session);
        shared.lock().upsert(&[ChatMsg {
            id: "inbound-1".into(),
            kind: "user".into(),
            content: "ping from Gauss".into(),
            from_label: Some("Gauss".into()),
            inbound_kind: Some("tab".into()),
            ..Default::default()
        }]);
        // Poll-loop flush of the reply (delta without the inbound row).
        shared.lock().upsert(&[ChatMsg {
            id: "reply-1".into(),
            kind: "assistant".into(),
            content: "pong".into(),
            ..Default::default()
        }]);

        let wiped = SnapshotBackend::with_messages(0); // live Map empty (refresh)
        let (msgs, total) = super::chatbackend_snapshot_from(&wiped, session, 0, &path).unwrap();
        assert_eq!(total, 2, "inbound + reply both survive to the snapshot");
        assert_eq!(msgs[0].content, "ping from Gauss");
        assert_eq!(msgs[0].from_label.as_deref(), Some("Gauss"));
        assert_eq!(msgs[1].content, "pong");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn chatbackend_snapshot_falls_back_to_live_poll_without_jsonl() {
        // disk ⇒ read the live plugin transcript.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.jsonl");
        let backend = SnapshotBackend::with_messages(2);
        let (msgs, total) = super::chatbackend_snapshot_from(&backend, "sid", 0, &missing).unwrap();
        assert_eq!(total, 2);
        assert_eq!(msgs[0].content, "m0");
    }

    #[test]
    fn live_chat_poll_interval_stays_low_latency() {
        assert!(live_chat_poll_interval() <= std::time::Duration::from_millis(150));
    }

    #[test]
    fn rich_text_and_tool_flatten() {
        let rich = RichMessage {
            role: "assistant".into(),
            blocks: vec![
                MessageBlock {
                    kind: "text".into(),
                    data: json!({"text": "hi"}),
                },
                MessageBlock {
                    kind: "tool_use".into(),
                    data: json!({"name": "Bash", "input": {"command": "codeterm tab list"}}),
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
        assert_eq!(
            msgs[1].tool_input.as_ref().unwrap()["command"],
            "codeterm tab list"
        );
    }

    #[test]
    fn canonical_tool_fields_passthrough_without_input_summary() {
        let rich = RichMessage {
            role: "assistant".into(),
            blocks: vec![
                MessageBlock {
                    kind: "tool_use".into(),
                    data: json!({
                        "name": "provider_specific_name",
                        "input": {"command": "must not become a summary"},
                        "tool_kind": "command",
                        "tool_summary": "Run command",
                        "tool_detail": "cargo check",
                        "tool_edits": [{"path": "src/lib.rs"}]
                    }),
                },
                MessageBlock {
                    kind: "tool_use".into(),
                    data: json!({
                        "name": "another_provider_name",
                        "input": {"command": "must not become JSON"}
                    }),
                },
            ],
            ..Default::default()
        };

        let msgs = rich_to_chat_msgs(&rich, 0);
        assert_eq!(msgs[0].tool_kind.as_deref(), Some("command"));
        assert_eq!(msgs[0].tool_summary.as_deref(), Some("Run command"));
        assert_eq!(msgs[0].tool_detail.as_deref(), Some("cargo check"));
        assert_eq!(
            msgs[0].tool_edits.as_ref().unwrap()[0]["path"],
            "src/lib.rs"
        );
        assert_eq!(msgs[0].content, "Run command");
        assert_eq!(msgs[1].tool_summary, None);
        assert_eq!(msgs[1].tool_detail, None);
        assert_eq!(msgs[1].tool_args, None);
        assert_eq!(msgs[1].content, "");
    }

    #[test]
    fn tool_activity_capabilities_and_flow_boundaries_pass_through() {
        let rich = RichMessage {
            role: "assistant".into(),
            blocks: vec![
                MessageBlock {
                    kind: "tool_flow".into(),
                    data: json!({ "state": "start", "flow_id": "turn-a" }),
                },
                MessageBlock {
                    kind: "tool_use".into(),
                    data: json!({
                        "name": "Bash",
                        "input": {"command": "npm test", "description": "Run tests"},
                        "tool_kind": "command",
                        "tool_summary": "npm test",
                        "tool_description": "Run tests",
                        "tool_title": "npm test",
                        "tool_flow_id": "msg-1",
                        "tool_echo": true
                    }),
                },
                MessageBlock {
                    kind: "tool_use".into(),
                    data: json!({ "name": "Read", "input": {"file_path": "a.rs"} }),
                },
                MessageBlock {
                    kind: "tool_flow".into(),
                    data: json!({ "state": "end" }),
                },
                MessageBlock {
                    kind: "tool_flow".into(),
                    data: json!({ "state": "guess", "flow_id": "turn-b" }),
                },
            ],
            ..Default::default()
        };
        let msgs = rich_to_chat_msgs(&rich, 0);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].kind, "tool_flow");
        assert_eq!(msgs[0].content, "start");
        assert_eq!(msgs[0].tool_flow_id.as_deref(), Some("turn-a"));
        assert_eq!(msgs[1].tool_description.as_deref(), Some("Run tests"));
        assert_eq!(msgs[1].tool_title.as_deref(), Some("npm test"));
        assert_eq!(msgs[1].tool_flow_id.as_deref(), Some("msg-1"));
        assert_eq!(msgs[1].tool_input.as_ref().unwrap()["command"], "npm test");
        assert_eq!(msgs[2].tool_description, None);
        assert_eq!(msgs[2].tool_title, None);
        assert_eq!(msgs[2].tool_flow_id, None);
        assert_eq!(msgs[1].tool_echo, Some(true));
        assert_eq!(msgs[2].tool_echo, None);
        assert_eq!(msgs[3].kind, "tool_flow");
        assert_eq!(msgs[3].content, "end");
        assert_eq!(msgs[3].tool_flow_id, None);
    }

    #[test]
    fn completed_tool_use_preserves_explicit_lifecycle() {
        let rich = RichMessage {
            role: "assistant".into(),
            blocks: vec![MessageBlock {
                kind: "tool_use".into(),
                data: json!({
                    "name": "Read",
                    "tool_kind": "read",
                    "tool_summary": "src/main.rs",
                    "tool_status": "success"
                }),
            }],
            ..Default::default()
        };
        let msgs = rich_to_chat_msgs(&rich, 0);
        assert_eq!(msgs[0].tool_error, Some(false));
        assert_eq!(msgs[0].tool_result, None);
    }

    #[test]
    fn tool_result_propagates_is_error_and_strips_ansi() {
        let rich = RichMessage {
            role: "assistant".into(),
            blocks: vec![MessageBlock {
                kind: "tool_result".into(),
                data: json!({
                    "tool_use_id": "tool-1",
                    "is_error": true,
                    "content": "\u{1b}[31mfailed\u{1b}[0m"
                }),
            }],
            ..Default::default()
        };
        let msgs = rich_to_chat_msgs(&rich, 0);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].kind, "tool_result");
        assert_eq!(msgs[0].tool_error, Some(true));
        assert_eq!(msgs[0].tool_result.as_deref(), Some("failed"));
    }

    #[test]
    fn user_turn_tool_result_propagates_is_error() {
        let rich = RichMessage {
            role: "user".into(),
            blocks: vec![MessageBlock {
                kind: "tool_result".into(),
                data: json!({
                    "tool_use_id": "tool-user-1",
                    "is_error": true,
                    "content": "boom"
                }),
            }],
            ..Default::default()
        };
        let msgs = rich_to_chat_msgs(&rich, 3);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].tool_id.as_deref(), Some("tool-user-1"));
        assert_eq!(msgs[0].tool_error, Some(true));
    }

    #[test]
    fn compact_summary_normalizes_to_collapsed_system_item() {
        let rich = RichMessage {
            role: "user".into(),
            blocks: vec![MessageBlock {
                kind: "text".into(),
                data: json!({"text": "This session is being continued from a previous conversation."}),
            }],
            uuid: Some("compact-user".into()),
            is_compact_summary: Some(true),
            is_visible_in_transcript_only: Some(true),
            source_type: Some("user".into()),
            ..Default::default()
        };

        let msgs = rich_to_chat_msgs(&rich, 7);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].kind, "system");
        assert_eq!(msgs[0].system_subtype.as_deref(), Some("compact_summary"));
        assert!(msgs[0].collapsed);
        assert_eq!(msgs[0].seq, 7);
    }

    #[test]
    fn compact_boundary_is_status_and_noisy_system_rows_are_hidden() {
        let boundary = RichMessage {
            role: "system".into(),
            blocks: vec![MessageBlock {
                kind: "text".into(),
                data: json!({"text": "", "subtype": "compact_boundary"}),
            }],
            uuid: Some("compact-boundary".into()),
            source_type: Some("system".into()),
            source_subtype: Some("compact_boundary".into()),
            ..Default::default()
        };
        let msgs = rich_to_chat_msgs(&boundary, 8);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].kind, "status");
        assert_eq!(msgs[0].content, "Context compacted");
        assert_eq!(msgs[0].system_subtype.as_deref(), Some("compact_boundary"));

        let turn_duration = RichMessage {
            role: "system".into(),
            blocks: vec![MessageBlock {
                kind: "text".into(),
                data: json!({"text": "123ms", "subtype": "turn_duration"}),
            }],
            source_type: Some("system".into()),
            source_subtype: Some("turn_duration".into()),
            ..Default::default()
        };
        assert!(rich_to_chat_msgs(&turn_duration, 9).is_empty());
    }

    #[test]
    fn provider_status_row_normalizes_to_status() {
        let rich = RichMessage {
            role: "status".into(),
            blocks: vec![MessageBlock {
                kind: "text".into(),
                data: json!({"text": "Task completed", "subtype": "task_notification"}),
            }],
            uuid: Some("task-notification".into()),
            source_type: Some("user".into()),
            source_subtype: Some("task_notification".into()),
            ..Default::default()
        };

        let msgs = rich_to_chat_msgs(&rich, 10);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].kind, "status");
        assert_eq!(msgs[0].content, "Task completed");
        assert_eq!(msgs[0].system_subtype.as_deref(), Some("task_notification"));
        assert_eq!(msgs[0].seq, 10);
    }
}
