use codeterm_types::agent::chat_types::{ChatAttachment, ChatMsg, InteractiveElement, RichMessage};

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

/// Flatten a JSONL RichMessage (role + typed blocks) into the normalized
/// `ChatMsg` shape — the single canonical chat shape shared with opencode's
/// structured path and the frontend `NormalizedChatMessage`. This is the one
/// place JSONL providers' rich rendering lives; the desktop renders the same
/// output, so there is no second parser.
pub fn rich_to_chat_msgs(rich: &RichMessage, base_id: usize) -> Vec<ChatMsg> {
    let mut out = rich_to_chat_msgs_blocks(rich, base_id);
    for m in &mut out {
        m.seq = base_id as i64;
    }
    out
}

fn rich_to_chat_msgs_blocks(rich: &RichMessage, base_id: usize) -> Vec<ChatMsg> {
    let mut out = Vec::new();
    let role = rich.role.as_str();
    let base = rich
        .uuid
        .clone()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| base_id.to_string());
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
                        tool_call_source_id: b.data.get("tool_call_source_id").and_then(|v| v.as_str()).map(String::from),
                        tool_pairing: b.data.get("tool_pairing").and_then(|v| v.as_str()).map(String::from),
                        tool_exit_code: b.data.get("tool_exit_code").and_then(|v| v.as_i64()),
                        tool_duration_ms: b.data.get("tool_duration_ms").and_then(|v| v.as_u64()),
                        tool_interrupted: b.data.get("tool_interrupted").and_then(|v| v.as_bool()),
                        tool_background_task_id: b.data.get("tool_background_task_id").and_then(|v| v.as_str()).map(String::from),
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
                let args = summarize_tool_input(b.data.get("input"));
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
                    content: format!("{name}({args})"),
                    tool_name: Some(name),
                    tool_args: Some(args),
                    tool_input: b.data.get("input").cloned(),
                    tool_id: tool_id.map(String::from),
                    tool_error,
                    tool_call_source_id: b.data.get("tool_call_source_id").and_then(|v| v.as_str()).map(String::from),
                    tool_pairing: b.data.get("tool_pairing").and_then(|v| v.as_str()).map(String::from),
                    tool_exit_code: b.data.get("tool_exit_code").and_then(|v| v.as_i64()),
                    tool_duration_ms: b.data.get("tool_duration_ms").and_then(|v| v.as_u64()),
                    tool_interrupted: b.data.get("tool_interrupted").and_then(|v| v.as_bool()),
                    tool_background_task_id: b.data.get("tool_background_task_id").and_then(|v| v.as_str()).map(String::from),
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
                    tool_call_source_id: b.data.get("tool_call_source_id").and_then(|v| v.as_str()).map(String::from),
                    tool_pairing: b.data.get("tool_pairing").and_then(|v| v.as_str()).map(String::from),
                    tool_exit_code: b.data.get("tool_exit_code").and_then(|v| v.as_i64()),
                    tool_duration_ms: b.data.get("tool_duration_ms").and_then(|v| v.as_u64()),
                    tool_interrupted: b.data.get("tool_interrupted").and_then(|v| v.as_bool()),
                    tool_background_task_id: b.data.get("tool_background_task_id").and_then(|v| v.as_str()).map(String::from),
                    collapsed: true,
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
