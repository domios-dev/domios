use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domios::{self, DOMIOS_CLOSE_TAG, DOMIOS_PASTED_MARKER};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageSegment {
    Text {
        text: String,
    },
    Frame {
        from: String,
        attrs: BTreeMap<String, String>,
        body: String,
        tip: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        segments: Vec<MessageSegment>,
    },
    Paste {
        id: Option<String>,
        body: String,
    },
    Attachment {
        kind: String,
        path: String,
        label: Option<String>,
        body: Option<String>,
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        excerpt: Option<String>,
    },
}

pub fn parse_envelope(text: &str) -> Vec<MessageSegment> {
    parse_nested(text, 0, true)
}

fn parse_nested(text: &str, depth: usize, allow_frames: bool) -> Vec<MessageSegment> {
    if depth >= 64 {
        return vec![MessageSegment::Text {
            text: text.to_string(),
        }];
    }
    let mut segments = Vec::new();
    let mut pending = String::new();
    let mut offset = 0;
    while offset < text.len() {
        if let Some(end) = quoted_end(text, offset) {
            pending.push_str(&text[offset..end]);
            offset = end;
            continue;
        }
        let line_start = offset == 0 || text.as_bytes()[offset - 1] == b'\n';
        let rest = &text[offset..];
        if line_start && rest.starts_with(crate::message_markers::OPEN) {
            if let Some(marker) = crate::message_markers::parse(rest) {
                let segment = match marker.kind.as_str() {
                    "voice" | "photo" | "file" if marker.attr("file").is_some() => {
                        Some(MessageSegment::Attachment {
                            kind: if marker.kind == "photo" {
                                "image".into()
                            } else {
                                marker.kind.clone()
                            },
                            path: marker.attr("file").unwrap_or_default().into(),
                            label: marker.attr("name").map(str::to_string),
                            body: Some(marker.payload.clone()),
                            status: marker.attr("status").map(str::to_string),
                            reference: None,
                            excerpt: None,
                        })
                    }
                    "plugin_config" | "system_prompt" if allow_frames => {
                        Some(MessageSegment::Frame {
                            from: marker.kind.clone(),
                            attrs: marker.attrs.into_iter().collect(),
                            body: marker.payload,
                            tip: None,
                            segments: Vec::new(),
                        })
                    }
                    _ => None,
                };
                if let Some(segment) = segment {
                    flush_text(&mut segments, &mut pending);
                    segments.push(segment);
                    offset = text.len();
                    continue;
                }
            }
        }
        let marked = line_start
            .then(|| rest.strip_prefix(DOMIOS_PASTED_MARKER))
            .flatten();
        let candidate = marked.map_or(rest, |tail| tail.trim_start_matches([' ', '\t']));
        let prefix_len = rest.len() - candidate.len();
        if allow_frames && (line_start || prefix_len > 0) && candidate.starts_with("<domios") {
            if let Some((segment, consumed)) = frame(candidate, depth) {
                flush_text(&mut segments, &mut pending);
                segments.push(segment);
                offset += prefix_len + consumed;
                continue;
            }
        }
        if line_start && rest.starts_with("<pasted_content") {
            if let Some((id, body, consumed)) = paste(rest) {
                flush_text(&mut segments, &mut pending);
                let inner = parse_nested(body, depth + 1, allow_frames);
                if inner
                    .iter()
                    .all(|segment| matches!(segment, MessageSegment::Text { .. }))
                {
                    let body = inner
                        .into_iter()
                        .filter_map(|segment| match segment {
                            MessageSegment::Text { text } => Some(text),
                            _ => None,
                        })
                        .collect::<String>();
                    segments.push(MessageSegment::Paste { id, body });
                } else {
                    segments.extend(inner);
                }
                offset += consumed;
                continue;
            }
        }
        if let Some((segment, consumed)) = attachment(rest) {
            flush_text(&mut segments, &mut pending);
            segments.push(segment);
            offset += consumed;
            continue;
        }
        if let Some(tail) = marked {
            if tail.trim_start_matches([' ', '\t', '\r']).starts_with('\n')
                || tail.trim().is_empty()
            {
                let consumed = rest.find('\n').map_or(rest.len(), |end| end + 1);
                offset += consumed;
                continue;
            }
        }
        let character = rest.chars().next().unwrap_or_default();
        pending.push(character);
        offset += character.len_utf8();
    }
    flush_text(&mut segments, &mut pending);
    segments
}

fn flush_text(segments: &mut Vec<MessageSegment>, pending: &mut String) {
    if pending.is_empty() {
        return;
    }
    segments.push(MessageSegment::Text {
        text: std::mem::take(pending),
    });
}

fn opening_end(text: &str) -> Option<usize> {
    let mut quoted = false;
    for (index, character) in text.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '>' if !quoted => return Some(index + 1),
            _ => {}
        }
    }
    None
}

fn frame(text: &str, recursion: usize) -> Option<(MessageSegment, usize)> {
    let opening = opening_end(text)?;
    if text[..opening].ends_with("/>") {
        return None;
    }
    let mut offset = opening;
    let mut depth = 1;
    while offset < text.len() {
        if let Some(end) = quoted_end(text, offset) {
            offset = end;
            continue;
        }
        let rest = &text[offset..];
        if rest.starts_with(DOMIOS_CLOSE_TAG) {
            depth -= 1;
            if depth == 0 {
                let consumed = offset + DOMIOS_CLOSE_TAG.len();
                let parsed = domios::parse_message(&text[..consumed])?;
                let from = parsed.attr("from")?.to_string();
                let tip = parsed
                    .content
                    .strip_suffix("</tip>")
                    .and_then(|source| source.rsplit_once("<tip>"))
                    .map(|(_, tip)| tip.to_string());
                let body = display_frame_body(&parsed.body);
                let mut segments = parse_nested(&body, recursion + 1, false);
                if segments
                    .iter()
                    .all(|segment| matches!(segment, MessageSegment::Text { .. }))
                {
                    segments.clear();
                }
                return Some((
                    MessageSegment::Frame {
                        from,
                        attrs: parsed.attrs,
                        body,
                        tip,
                        segments,
                    },
                    consumed,
                ));
            }
            offset += DOMIOS_CLOSE_TAG.len();
        } else if rest.starts_with("<domios ") {
            let end = opening_end(rest)?;
            if !rest[..end].ends_with("/>") {
                depth += 1;
            }
            offset += end;
        } else {
            offset += rest.chars().next()?.len_utf8();
        }
    }
    None
}

fn clean_markers(text: &str) -> String {
    let mut out = String::new();
    let mut offset = 0;
    while offset < text.len() {
        if let Some(end) = quoted_end(text, offset) {
            out.push_str(&text[offset..end]);
            offset = end;
            continue;
        }
        let end = text[offset..]
            .find('\n')
            .map_or(text.len(), |end| offset + end + 1);
        let line = &text[offset..end];
        if line.trim_end() != DOMIOS_PASTED_MARKER {
            out.push_str(line);
        }
        offset = end;
    }
    out
}

fn display_frame_body(text: &str) -> String {
    let mut body = String::new();
    let mut offset = 0;
    while offset < text.len() {
        if let Some(end) = quoted_end(text, offset) {
            body.push_str(&text[offset..end]);
            offset = end;
            continue;
        }
        let rest = &text[offset..];
        if let Some(inner) = rest.strip_prefix("<tip>") {
            if let Some(end) = inner.find("</tip>") {
                offset += "<tip>".len() + end + "</tip>".len();
                continue;
            }
        }
        if let Some((_, pasted, consumed)) = paste(rest) {
            body.push_str(pasted);
            offset += consumed;
            continue;
        }
        let character = rest.chars().next().unwrap_or_default();
        body.push(character);
        offset += character.len_utf8();
    }
    clean_markers(&body)
}

fn attachment(text: &str) -> Option<(MessageSegment, usize)> {
    let rest = text.strip_prefix("<domios ")?;
    let mut end = opening_end(text)?;
    let attributes = domios::parse_attributes(
        rest[..end - "<domios ".len()]
            .strip_suffix("/>")?
            .trim_end(),
    )?;
    if attributes.get("type")? != "attachment" || attributes.contains_key("from") {
        return None;
    }
    let kind = attributes.get("kind")?.clone();
    if attributes.get("path")?.is_empty() {
        return None;
    }
    if ![
        "voice", "image", "video", "file", "text", "folder", "quote", "message", "diff", "document",
    ]
    .contains(&kind.as_str())
    {
        return None;
    }
    let body = if kind == "voice" {
        let tail = text[end..]
            .strip_prefix("\r\n")
            .or_else(|| text[end..].strip_prefix('\n'))
            .unwrap_or(&text[end..]);
        if tail.starts_with("<transcript>") {
            let close = tail.find("</transcript>")? + "</transcript>".len();
            end = text.len() - tail.len() + close;
            Some(crate::message_markers::parse(&text[..end])?.payload)
        } else {
            None
        }
    } else {
        None
    };
    Some((
        MessageSegment::Attachment {
            kind,
            path: attributes.get("path")?.clone(),
            label: attributes.get("label").cloned(),
            body,
            status: attributes.get("status").cloned(),
            reference: attributes.get("reply").cloned(),
            excerpt: attributes.get("excerpt").cloned(),
        },
        end,
    ))
}

fn paste(text: &str) -> Option<(Option<String>, &str, usize)> {
    let rest = text.strip_prefix("<pasted_content")?;
    if !rest.starts_with([' ', '>']) {
        return None;
    }
    let end = opening_end(text)?;
    let attrs = domios::parse_attributes(&text["<pasted_content".len()..end - 1])?;
    let close = text[end..].find("</pasted_content")? + end;
    let close_end = opening_end(&text[close..])? + close;
    let close_attrs =
        domios::parse_attributes(&text[close + "</pasted_content".len()..close_end - 1])?;
    if close_attrs
        .get("id")
        .is_some_and(|id| Some(id) != attrs.get("id"))
    {
        return None;
    }
    Some((attrs.get("id").cloned(), &text[end..close], close_end))
}

fn quoted_end(text: &str, offset: usize) -> Option<usize> {
    let rest = &text[offset..];
    let line_start = offset == 0 || text.as_bytes()[offset - 1] == b'\n';
    if line_start {
        let line = rest.lines().next().unwrap_or_default();
        if line.starts_with("    ") || line.starts_with('\t') || line.starts_with('>') {
            return Some(offset + rest.find('\n').map_or(rest.len(), |end| end + 1));
        }
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() <= 3 {
            let fence_char = trimmed.chars().next()?;
            if matches!(fence_char, '`' | '~') {
                let count = trimmed
                    .chars()
                    .take_while(|character| *character == fence_char)
                    .count();
                if count >= 3 {
                    let mut position = offset + line.len();
                    for next in text[position..].split_inclusive('\n') {
                        let candidate = next.trim();
                        position += next.len();
                        if candidate.chars().count() >= count
                            && candidate.chars().all(|character| character == fence_char)
                        {
                            return Some(position);
                        }
                    }
                    return Some(text.len());
                }
            }
        }
    }
    for tag in ["tool_result", "tool_output", "transcript"] {
        if rest.starts_with(&format!("<{tag}>")) || rest.starts_with(&format!("<{tag} ")) {
            let close = format!("</{tag}>");
            return Some(
                rest.find(&close)
                    .map_or(text.len(), |end| offset + end + close.len()),
            );
        }
    }
    if rest.starts_with('`') {
        let count = rest.bytes().take_while(|byte| *byte == b'`').count();
        let delimiter = &rest[..count];
        return rest[count..]
            .find(delimiter)
            .map(|end| offset + count + end + count);
    }
    None
}

#[cfg(test)]
mod tests;
