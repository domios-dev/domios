use std::ops::Range;

use super::{parse_nested, FrameKind, MessageSegment};
use crate::domios::{self, DOMIOS_CLOSE_TAG, DOMIOS_PASTED_MARKER};

const FRAME_OPEN: &str = "<domios";
const TIP_OPEN: &str = "<tip>";
const TIP_CLOSE: &str = "</tip>";
const PASTE_OPEN: &str = "<pasted_content";
const PASTE_CLOSE: &str = "</pasted_content";

pub(super) fn opens_frame(text: &str) -> bool {
    text.strip_prefix(FRAME_OPEN).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_none_or(|next| matches!(next, ' ' | '\t' | '\r' | '\n' | '>' | '/'))
    })
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

/// Bytes up to and including the close that balances the leading opening tag;
/// `None` while that close has not arrived.
pub(super) fn frame_extent(text: &str) -> Option<usize> {
    let opening = opening_end(text)?;
    if text[..opening].ends_with("/>") {
        return Some(opening);
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
            offset += DOMIOS_CLOSE_TAG.len();
            if depth == 0 {
                return Some(offset);
            }
        } else if opens_frame(rest) {
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

fn opening_attributes(tag: &str) -> Option<std::collections::BTreeMap<String, String>> {
    let inner = tag.strip_prefix(FRAME_OPEN)?;
    let inner = inner
        .strip_suffix("/>")
        .or_else(|| inner.strip_suffix('>'))?;
    domios::parse_attributes(inner)
}

pub(super) fn frame_segment(raw: &str, depth: usize) -> MessageSegment {
    let opening = opening_end(raw).unwrap_or(raw.len());
    let self_closing = raw[..opening].ends_with("/>");
    let attrs = opening_attributes(&raw[..opening]);
    let content = if self_closing {
        ""
    } else {
        raw[opening..]
            .strip_suffix(DOMIOS_CLOSE_TAG)
            .unwrap_or(&raw[opening..])
    };
    let body = display_body(content, false);
    let kind = attrs
        .as_ref()
        .and_then(|attrs| attrs.get("from"))
        .and_then(|from| FrameKind::from_wire(from));
    match (attrs, kind) {
        (Some(attrs), Some(kind)) if !self_closing => {
            let mut segments = parse_nested(&body, depth + 1, false)
                .into_iter()
                .map(|spanned| spanned.segment)
                .collect::<Vec<_>>();
            if segments.iter().all(MessageSegment::is_text) {
                segments.clear();
            }
            MessageSegment::Frame {
                kind,
                from: kind.as_str().to_string(),
                attrs,
                tip: trailing_tip(content),
                body,
                segments,
            }
        }
        (attrs, _) => MessageSegment::UnknownFrame {
            raw: raw.to_string(),
            attrs: attrs.unwrap_or_default(),
            body,
        },
    }
}

pub(super) fn partial_frame(raw: &str) -> MessageSegment {
    let (attrs, body) = match opening_end(raw) {
        Some(opening) => (
            opening_attributes(&raw[..opening]).unwrap_or_default(),
            display_body(&raw[opening..], true),
        ),
        None => (Default::default(), String::new()),
    };
    MessageSegment::PartialFrame {
        raw: raw.to_string(),
        attrs,
        body,
    }
}

fn trailing_tip(content: &str) -> Option<String> {
    content
        .trim_end()
        .strip_suffix(TIP_CLOSE)
        .and_then(|source| source.rsplit_once(TIP_OPEN))
        .map(|(_, tip)| tip.to_string())
}

/// The sender's words without host markup: tips, paste wrappers and typed
/// markers removed. A partial body also drops a tag cut off mid-stream.
fn display_body(content: &str, partial: bool) -> String {
    let mut body = String::new();
    let mut offset = 0;
    while offset < content.len() {
        if let Some(end) = quoted_end(content, offset) {
            body.push_str(&content[offset..end]);
            offset = end;
            continue;
        }
        let rest = &content[offset..];
        if let Some(inner) = rest.strip_prefix(TIP_OPEN) {
            match inner.find(TIP_CLOSE) {
                Some(end) => {
                    offset += TIP_OPEN.len() + end + TIP_CLOSE.len();
                    continue;
                }
                None if partial => break,
                None => {}
            }
        }
        if let Some(found) = paste(rest) {
            body.push_str(&rest[found.body]);
            offset += found.consumed;
            continue;
        }
        let character = rest.chars().next().unwrap_or_default();
        body.push(character);
        offset += character.len_utf8();
    }
    if partial {
        if let Some(cut) = body.rfind('<').filter(|&cut| {
            !body[cut..].contains('>')
                && body[cut + 1..]
                    .chars()
                    .next()
                    .is_none_or(|next| next.is_ascii_alphabetic() || next == '/')
        }) {
            body.truncate(cut);
        }
    }
    let cleaned = without_marker_lines(&body);
    let trimmed = cleaned.trim_end();
    let first_content_line = trimmed
        .char_indices()
        .find(|(_, character)| !character.is_whitespace())
        .map_or(trimmed.len(), |(index, _)| {
            trimmed[..index].rfind('\n').map_or(0, |line| line + 1)
        });
    trimmed[first_content_line..].to_string()
}

fn without_marker_lines(text: &str) -> String {
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
        if line.trim() != DOMIOS_PASTED_MARKER {
            out.push_str(line);
        }
        offset = end;
    }
    out
}

pub(super) fn attachment(text: &str) -> Option<(MessageSegment, usize)> {
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

pub(super) struct FoundPaste {
    pub(super) id: Option<String>,
    pub(super) body: Range<usize>,
    pub(super) consumed: usize,
}

/// A paste wrapper; an unclosed one runs to the end of the input.
pub(super) fn paste(text: &str) -> Option<FoundPaste> {
    let rest = text.strip_prefix(PASTE_OPEN)?;
    if !rest.starts_with([' ', '>']) {
        return None;
    }
    let end = opening_end(text)?;
    let attrs = domios::parse_attributes(&text[PASTE_OPEN.len()..end - 1])?;
    let id = attrs.get("id").cloned();
    let unclosed = FoundPaste {
        id: id.clone(),
        body: end..text.len(),
        consumed: text.len(),
    };
    let Some(close) = text[end..].find(PASTE_CLOSE).map(|close| close + end) else {
        return Some(unclosed);
    };
    let Some(close_end) = opening_end(&text[close..]).map(|close_end| close_end + close) else {
        return Some(unclosed);
    };
    let close_attrs = domios::parse_attributes(&text[close + PASTE_CLOSE.len()..close_end - 1])?;
    if close_attrs
        .get("id")
        .is_some_and(|close_id| Some(close_id) != id.as_ref())
    {
        return None;
    }
    Some(FoundPaste {
        id,
        body: end..close,
        consumed: close_end,
    })
}

pub(super) fn quoted_end(text: &str, offset: usize) -> Option<usize> {
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
