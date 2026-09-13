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
                        file_diffs: b.data.get("file_diffs").cloned(),
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
                let file_diffs = b.data.get("file_diffs").cloned();
                let tool_rich_result = b.data.get("tool_rich_result").filter(|v| v.is_object()).cloned();
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
                    file_diffs,
                    tool_rich_result,
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
                    file_diffs: b.data.get("file_diffs").cloned(),
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
mod tests;
