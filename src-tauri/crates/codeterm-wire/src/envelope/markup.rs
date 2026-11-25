use std::ops::Range;

use super::{parse_nested, FrameKind, MessageSegment};
use crate::domios::{self, DOMIOS_CLOSE_TAG, DOMIOS_PASTED_MARKER};

const FRAME_OPEN: &str = "<domios";
const TIP_OPEN: &str = "<tip>";
const TIP_CLOSE: &str = "</tip>";
const PASTE_OPEN: &str = "<pasted_content";
const PASTE_CLOSE: &str = "</pasted_content";

enum OpeningScan {
    Complete {
        end: usize,
        self_closing: bool,
        has_from: bool,
    },
    Truncated {
        has_from: bool,
        has_non_from: bool,
        inline_prefix: bool,
    },
    Inert,
}

fn inline_sender_prefix(completed: &str, pending: &str) -> bool {
    let Some(attrs) = domios::parse_attributes(completed) else {
        return false;
    };
    let has_sender = attrs.get("from").is_some_and(|value| !value.is_empty());
    let Some((name, value)) = pending.split_once('=') else {
        return has_sender;
    };
    if attrs.contains_key(name) {
        return false;
    }
    if value.is_empty() {
        return has_sender || name == "from";
    }
    // Inside an open quote, arbitrary prose is indistinguishable from an attribute value.
    name == "from"
        && value.strip_prefix('"').is_some_and(|prefix| {
            FrameKind::ALL
                .iter()
                .any(|kind| kind.as_str().starts_with(prefix))
        })
}

fn scan_domios_opening(text: &str) -> OpeningScan {
    if !text.starts_with(FRAME_OPEN) {
        return OpeningScan::Inert;
    }
    let bytes = text.as_bytes();
    let mut pos = FRAME_OPEN.len();
    let mut has_from = false;
    let mut has_non_from = false;
    let truncated = |has_from, has_non_from, pending: usize| OpeningScan::Truncated {
        has_from,
        has_non_from,
        inline_prefix: has_from
            && inline_sender_prefix(&text[FRAME_OPEN.len()..pending], &text[pending..]),
    };
    loop {
        let ws_start = pos;
        while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t' | b'\r' | b'\n') {
            pos += 1;
        }
        if pos == bytes.len() {
            return truncated(has_from, has_non_from, pos);
        }
        if bytes[pos] == b'>' {
            return OpeningScan::Complete {
                end: pos + 1,
                self_closing: false,
                has_from,
            };
        }
        if bytes[pos] == b'/' {
            if pos + 1 == bytes.len() {
                return OpeningScan::Inert;
            }
            if bytes[pos + 1] == b'>' {
                return OpeningScan::Complete {
                    end: pos + 2,
                    self_closing: true,
                    has_from,
                };
            }
            return OpeningScan::Inert;
        }
        if pos == ws_start {
            return OpeningScan::Inert;
        }
        if !bytes[pos].is_ascii_alphabetic() {
            return OpeningScan::Inert;
        }
        let name_start = pos;
        while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
            pos += 1;
        }
        let name = &text[name_start..pos];
        if pos == bytes.len() {
            let could_be_from = "from".starts_with(name);
            return truncated(has_from, has_non_from || !could_be_from, name_start);
        }
        if bytes[pos] != b'=' {
            return OpeningScan::Inert;
        }
        if name == "from" {
            has_from = true;
        } else {
            has_non_from = true;
        }
        pos += 1;
        if pos == bytes.len() {
            return truncated(has_from, has_non_from, name_start);
        }
        if bytes[pos] == b'"' {
            pos += 1;
            let Some(rel_end) = text[pos..].find('"') else {
                if text[pos..].contains('<') {
                    return OpeningScan::Inert;
                }
                return truncated(has_from, has_non_from, name_start);
            };
            pos += rel_end + 1;
        } else {
            let val_start = pos;
            while pos < bytes.len() {
                let b = bytes[pos];
                if matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'>') {
                    break;
                }
                if b == b'/' && bytes.get(pos + 1) == Some(&b'>') {
                    break;
                }
                if matches!(b, b'"' | b'\'' | b'<' | b'`' | b'=') {
                    return OpeningScan::Inert;
                }
                pos += 1;
            }
            if pos == val_start {
                return OpeningScan::Inert;
            }
        }
        if pos == bytes.len() {
            return truncated(has_from, has_non_from, pos);
        }
        if !matches!(bytes[pos], b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/') {
            return OpeningScan::Inert;
        }
    }
}

pub(super) fn opens_frame(text: &str) -> bool {
    matches!(
        scan_domios_opening(text),
        OpeningScan::Complete {
            self_closing: false,
            has_from: true,
            ..
        } | OpeningScan::Truncated { has_from: true, .. }
            | OpeningScan::Truncated {
                has_non_from: false,
                ..
            }
    )
}

pub(super) fn frame_candidate(
    rest: &str,
    boundary: bool,
    line_start: bool,
    depth: usize,
) -> Option<(MessageSegment, usize)> {
    match scan_domios_opening(rest) {
        OpeningScan::Complete {
            end: opening,
            self_closing: false,
            has_from: true,
        } => match frame_extent(rest, opening, line_start) {
            Some(consumed) => Some((frame_segment(&rest[..consumed], opening, depth), consumed)),
            None if boundary => Some((partial_frame(rest, Some(opening)), rest.len())),
            None => None,
        },
        OpeningScan::Truncated {
            has_from,
            has_non_from,
            inline_prefix,
        } if (boundary && (has_from || !has_non_from)) || inline_prefix => {
            Some((partial_frame(rest, None), rest.len()))
        }
        _ => None,
    }
}

fn opening_end(text: &str) -> Option<usize> {
    let mut quoted = false;
    for (index, character) in text.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '<' if index > 0 && !quoted => return None,
            '>' if !quoted => return Some(index + 1),
            _ => {}
        }
    }
    None
}

fn frame_extent(text: &str, opening: usize, line_start: bool) -> Option<usize> {
    let mut offset = opening;
    let mut within_nested_sender = Vec::new();
    let mut first_literal_close = None;
    while offset < text.len() {
        if let Some(end) = quoted_end(text, offset) {
            offset = end;
            continue;
        }
        let rest = &text[offset..];
        if rest.starts_with(DOMIOS_CLOSE_TAG) {
            offset += DOMIOS_CLOSE_TAG.len();
            match within_nested_sender.pop() {
                None => return Some(offset),
                Some(false) if line_start && first_literal_close.is_none() => {
                    first_literal_close = Some(offset);
                }
                _ => {}
            }
        } else if rest.starts_with(FRAME_OPEN) {
            match scan_domios_opening(rest) {
                OpeningScan::Complete {
                    end,
                    self_closing,
                    has_from,
                } => {
                    if !self_closing {
                        within_nested_sender
                            .push(has_from || within_nested_sender.last() == Some(&true));
                    }
                    offset += end;
                }
                OpeningScan::Truncated { has_from: true, .. } => break,
                OpeningScan::Truncated {
                    has_from: false, ..
                }
                | OpeningScan::Inert => {
                    offset += rest.chars().next()?.len_utf8();
                }
            }
        } else {
            offset += rest.chars().next()?.len_utf8();
        }
    }
    first_literal_close
}

fn opening_attributes(tag: &str) -> Option<std::collections::BTreeMap<String, String>> {
    let inner = tag.strip_prefix(FRAME_OPEN)?;
    let inner = inner
        .strip_suffix("/>")
        .or_else(|| inner.strip_suffix('>'))?;
    domios::parse_attributes(inner)
}

fn frame_segment(raw: &str, opening: usize, depth: usize) -> MessageSegment {
    let attrs = opening_attributes(&raw[..opening]);
    let content = &raw[opening..raw.len() - DOMIOS_CLOSE_TAG.len()];
    let body = display_body(content, false);
    let kind = attrs
        .as_ref()
        .and_then(|attrs| attrs.get("from"))
        .and_then(|from| FrameKind::from_wire(from));
    match (attrs, kind) {
        (Some(attrs), Some(kind)) => {
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

fn partial_frame(raw: &str, opening: Option<usize>) -> MessageSegment {
    let (attrs, body) = match opening {
        Some(opening) => (
            opening_attributes(&raw[..opening]).unwrap_or_default(),
            display_body(&raw[opening..], true),
        ),
        None => (Default::default(), raw.to_string()),
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
    if let Some(end) = context_end(rest) {
        return Some(offset + end);
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

const CONTEXT_TAGS: &[&str] = &[
    "context",
    "quoted-context",
    "quoted_context",
    "handoff",
    "provider-handoff",
    "provider_handoff",
    "environment_context",
    "system-reminder",
    "instructions",
    "INSTRUCTIONS",
    "previous_messages",
    "conversation_history",
];

fn quoted_tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (at, c) in text.char_indices() {
        if let Some(open) = quote {
            if c == open {
                quote = None;
            }
        } else if matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if c == '>' {
            return Some(at + 1);
        }
    }
    None
}

fn tag_name(text: &str) -> Option<(&str, bool)> {
    let rest = text.strip_prefix('<')?;
    let closing = rest.starts_with('/');
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    if !rest.as_bytes().first()?.is_ascii_alphabetic() {
        return None;
    }
    let width = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(*b, b'-' | b'_'))
        .count();
    if width == 0
        || !rest
            .as_bytes()
            .get(width)
            .is_some_and(|b| b.is_ascii_whitespace() || matches!(*b, b'/' | b'>'))
    {
        return None;
    }
    Some((&rest[..width], closing))
}

fn context_end(text: &str) -> Option<usize> {
    let (name, closing) = tag_name(text)?;
    if closing || !CONTEXT_TAGS.contains(&name) {
        return None;
    }
    let Some(mut at) = quoted_tag_end(text) else {
        return Some(text.len());
    };
    if text[..at].ends_with("/>") {
        return Some(at);
    }
    let mut stack = vec![name];
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with("<!--") {
            if rest.starts_with("<!-->") {
                at += 5;
                continue;
            }
            let close = ["-->", "--!>"]
                .into_iter()
                .filter_map(|end| rest.find(end).map(|index| index + end.len()))
                .min();
            let Some(close) = close else {
                return Some(text.len());
            };
            at += close;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let Some(end) = rest.find("]]>") else {
                return Some(text.len());
            };
            at += end + 3;
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            let Some(end) = quoted_tag_end(rest) else {
                return Some(text.len());
            };
            at += end;
            continue;
        }
        if rest.starts_with('`') {
            let width = rest.bytes().take_while(|b| *b == b'`').count();
            if width < 3 {
                if let Some(end) = rest[width..].find(&rest[..width]) {
                    at += width + end + width;
                    continue;
                }
            }
            at += width;
            continue;
        }
        let Some((inner, closing)) = tag_name(rest) else {
            at += rest.chars().next()?.len_utf8();
            continue;
        };
        let Some(end) = quoted_tag_end(rest) else {
            return Some(text.len());
        };
        let token = &rest[..end];
        if CONTEXT_TAGS.contains(&inner) {
            if closing {
                if stack.last() != Some(&inner)
                    || !token[2 + inner.len()..token.len() - 1].trim().is_empty()
                {
                    return Some(text.len());
                }
                stack.pop();
                if stack.is_empty() {
                    return Some(at + end);
                }
            } else if !token.ends_with("/>") {
                stack.push(inner);
            }
        } else if !closing
            && [
                "script",
                "style",
                "title",
                "textarea",
                "xmp",
                "iframe",
                "noembed",
                "noframes",
                "noscript",
                "plaintext",
            ]
            .iter()
            .any(|name| inner.eq_ignore_ascii_case(name))
        {
            if inner.eq_ignore_ascii_case("plaintext") {
                return Some(text.len());
            }
            let tail = &rest[end..];
            let close = tail.char_indices().find_map(|(index, _)| {
                let (name, closing) = tag_name(&tail[index..])?;
                (closing && name.eq_ignore_ascii_case(inner)).then_some(index)
            });
            let Some(close) = close else {
                return Some(text.len());
            };
            let Some(close_end) = quoted_tag_end(&tail[close..]) else {
                return Some(text.len());
            };
            at += end + close + close_end;
            continue;
        }
        at += end;
    }
    Some(text.len())
}
