use super::{
    file_paths, html::HtmlBoundary, space, ArtifactScan, FileCard, HostArtifactProvenance, KINDS,
};
use codeterm_wire::envelope::{parse_envelope_spans, MessageSegment};
use regex::Regex;
use std::sync::LazyLock;

const CONTEXTS: &[&str] = &[
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
    "tool_result",
    "tool_output",
    "transcript",
    "pasted_content",
];
static FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ {0,3}(`{3,}|~{3,})([^\r\n]*)$").unwrap());
static CONTEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ {0,3}</?([A-Za-z_-]+)(?:\s[^>]*)?>").unwrap());
static LIST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ {0,3}(?:[-+*]\s+|\d+[.)]\s+|> ?)").unwrap());
static RAW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^ {0,3}<(?:/?(?:address|article|aside|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|nav|ol|p|pre|script|section|source|style|summary|table|tbody|td|textarea|tfoot|th|thead|title|tr|track|ul|xmp|noembed|noframes|noscript|plaintext)(?:[\t\r\n />])|[!?])").unwrap()
});
static OPAQUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^ {0,3}<(?:script|style|title|textarea|xmp|iframe|noembed|noframes|noscript|plaintext)(?:[\t\r\n />])|^ {0,3}<[!?]").unwrap()
});

fn candidate(source: &str, start: usize, kind: &str) -> Option<(usize, Option<Vec<String>>)> {
    let mut quote = None;
    let mut header_end = None;
    for (offset, c) in source[start..].char_indices() {
        if let Some(open) = quote {
            if c == open {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if c == '>' {
            header_end = Some(start + offset + 1);
            break;
        }
    }
    let end = header_end?;
    let header = &source[start..end];
    if header.ends_with("/>") {
        return Some((
            end,
            if matches!(kind, "image" | "page" | "document") {
                file_paths(header, kind)
            } else {
                None
            },
        ));
    }
    let mut boundary = HtmlBoundary::new(&format!("ds-{kind}"));
    let mut json_quote = false;
    let mut escaped = false;
    for (offset, c) in source[end..].char_indices() {
        if kind == "tasks" {
            if json_quote {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    json_quote = false;
                }
                continue;
            }
            if c == '"' {
                json_quote = true;
                continue;
            }
        }
        if boundary.feed(c) {
            return Some((end + offset + c.len_utf8(), None));
        }
    }
    None
}

fn frame_body_start(source: &str) -> usize {
    let mut quote = None;
    for (at, c) in source.char_indices() {
        if let Some(open) = quote {
            if c == open {
                quote = None;
            }
        } else if matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if c == '>' {
            return at + 1;
        }
    }
    source.len()
}

/// Scans direct author text; routed wrappers remain inert without host provenance.
pub fn scan_artifacts(source: &str, trusted: bool) -> ArtifactScan {
    scan(source, trusted, None)
}

/// Scans only the source matched by independently supplied host provenance.
pub fn scan_artifacts_with_provenance(
    source: &str,
    provenance: &HostArtifactProvenance<'_>,
) -> ArtifactScan {
    if !provenance.matches_source(source) {
        return ArtifactScan::default();
    }
    scan(source, true, Some(provenance))
}

fn scan(
    source: &str,
    trusted: bool,
    provenance: Option<&HostArtifactProvenance<'_>>,
) -> ArtifactScan {
    let mut result = ArtifactScan::default();
    if !trusted {
        return result;
    }
    let mut body_start = 0;
    let excluded: Vec<_> = parse_envelope_spans(source)
        .into_iter()
        .filter_map(|part| {
            let live = match &part.segment {
                MessageSegment::Text { .. } | MessageSegment::Attachment { .. } => true,
                MessageSegment::Frame { .. } => {
                    let live = part.span.start == 0
                        && source.starts_with("<domios")
                        && source[part.span.end..].trim().is_empty()
                        && provenance.is_some_and(|evidence| evidence.permits_frame(&part.segment));
                    if live {
                        body_start = frame_body_start(source);
                    }
                    live
                }
                _ => false,
            };
            (!live).then_some(part.span)
        })
        .collect();
    let mut at = body_start;
    let mut fence: Option<(u8, usize)> = None;
    let mut contexts: Vec<String> = Vec::new();
    let mut list = false;
    let mut list_indent = 0;
    let mut list_blank = false;
    let mut inline_ticks = 0;
    let mut raw_html: Option<HtmlBoundary> = None;
    let mut opaque = false;
    let mut count = 0;
    let mut consumed: Vec<std::ops::Range<usize>> = Vec::new();
    while at < source.len() {
        if at > 0 && at != body_start && source.as_bytes()[at - 1] != b'\n' {
            at = source[at..]
                .find('\n')
                .map_or(source.len(), |offset| at + offset + 1);
            continue;
        }
        let end = source[at..]
            .find('\n')
            .map_or(source.len(), |offset| at + offset + 1);
        let raw_line = &source[at..end];
        let line = raw_line
            .strip_suffix("\r\n")
            .or_else(|| raw_line.strip_suffix('\n'))
            .unwrap_or(raw_line);
        let marker = FENCE.captures(line);
        if let Some((open, width)) = fence {
            if let Some(marker) = &marker {
                if marker[1].as_bytes()[0] == open
                    && marker[1].len() >= width
                    && marker[2].trim().is_empty()
                {
                    fence = None;
                }
            }
            at = end;
            continue;
        }
        if inline_ticks == 0 && contexts.is_empty() && raw_html.is_none() {
            if let Some(marker) = &marker {
                fence = Some((marker[1].as_bytes()[0], marker[1].len()));
                at = end;
                continue;
            }
        }
        if contexts.is_empty()
            && inline_ticks == 0
            && !list
            && raw_html.is_none()
            && RAW.is_match(line)
        {
            raw_html = Some(HtmlBoundary::new("ds-html"));
            opaque = OPAQUE.is_match(line);
        }
        let inside_html = raw_html.is_some();
        let html_data = raw_html.as_ref().is_none_or(HtmlBoundary::in_data);
        if let Some(context) = CONTEXT.captures(line) {
            let name = &context[1];
            if CONTEXTS.contains(&name)
                || (name == "domios" && (body_start > 0 || at != 0) && !context[0].ends_with("/>"))
            {
                if line.trim_start().starts_with("</") {
                    if contexts.last().is_some_and(|open| open == name) {
                        contexts.pop();
                    }
                    if contexts.is_empty() {
                        inline_ticks = 0;
                    }
                } else {
                    contexts.push(name.into());
                    if line.contains(&format!("</{name}>")) {
                        contexts.pop();
                    }
                }
            }
        }
        if line.trim().is_empty() {
            inline_ticks = 0;
            if list_indent > 0 {
                list_blank = true;
            } else {
                list = false;
            }
        }
        let indent = line.bytes().take_while(|b| *b == b' ').count();
        if !line.trim().is_empty() && list_blank {
            if indent < list_indent {
                list = false;
            }
            list_blank = false;
        }
        let list_marker = LIST.find(line);
        if let Some(marker) = &list_marker {
            list = true;
            list_indent = if marker.as_str().trim_start().starts_with('>') {
                0
            } else {
                marker.end()
            };
        }
        let literal_indent = list_marker.as_ref().map_or(indent.min(3), |m| m.end());
        let rest = &line[literal_indent..];
        if contexts.is_empty()
            && !list
            && !inside_html
            && inline_ticks == 0
            && indent <= 3
            && !line.starts_with('\t')
        {
            for part in parse_envelope_spans(line) {
                if let MessageSegment::Attachment { path, .. } = part.segment {
                    let start = at + part.span.start;
                    let end = at + part.span.end;
                    if !excluded.iter().any(|range| {
                        start < range.end
                            && end > range.start
                            && !consumed
                                .iter()
                                .any(|span| span.start < range.start && range.start < span.end)
                    }) {
                        result.attachment_paths.push(path);
                    }
                }
            }
        }
        if !opaque
            && html_data
            && contexts.is_empty()
            && inline_ticks == 0
            && indent <= 3
            && !line.starts_with('\t')
        {
            if let Some(after) = rest.strip_prefix("<ds-") {
                let width = after.bytes().take_while(u8::is_ascii_lowercase).count();
                let kind = &after[..width];
                if KINDS.contains(&kind)
                    && after
                        .as_bytes()
                        .get(width)
                        .is_none_or(|b| space(*b) || matches!(*b, b'/' | b'>'))
                {
                    let start = at + literal_indent;
                    let Some((next, paths)) = candidate(source, start, kind) else {
                        break;
                    };
                    consumed.push(start..next);
                    if !list
                        && !inside_html
                        && !excluded.iter().any(|range| {
                            start < range.end
                                && next > range.start
                                && !consumed
                                    .iter()
                                    .any(|span| span.start < range.start && range.start < span.end)
                        })
                    {
                        if let Some(paths) = paths {
                            count += paths.len();
                            if count <= 32 {
                                result.cards.push(FileCard {
                                    start,
                                    end: next,
                                    kind: kind.into(),
                                    paths,
                                });
                            } else {
                                result.overflow = true;
                            }
                        }
                    }
                    at = next;
                    continue;
                }
            }
        }
        if let Some(boundary) = raw_html.as_mut() {
            for c in source[at..end].chars() {
                boundary.feed(c);
            }
            if boundary.in_data() && (opaque || line.trim().is_empty()) {
                raw_html = None;
                opaque = false;
            }
        }
        if contexts.is_empty() && !inside_html {
            let bytes = line.as_bytes();
            let mut pos = 0;
            while pos < bytes.len() {
                if bytes[pos] == b'`' && (pos == 0 || bytes[pos - 1] != b'\\') {
                    let width = bytes[pos..].iter().take_while(|b| **b == b'`').count();
                    if inline_ticks == 0 {
                        inline_ticks = width;
                    } else if inline_ticks == width {
                        inline_ticks = 0;
                    }
                    pos += width;
                } else {
                    pos += 1;
                }
            }
        }
        at = end;
    }
    result
}
