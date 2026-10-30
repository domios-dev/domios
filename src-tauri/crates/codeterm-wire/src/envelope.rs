use std::collections::BTreeMap;
use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::domios::DOMIOS_PASTED_MARKER;

mod markup;

use markup::{
    attachment, frame_extent, frame_segment, opens_frame, partial_frame, paste, quoted_end,
};

/// Bumped whenever a segment shape or a parse decision changes, so a client
/// built against another grammar can refuse rather than misrender.
pub const ENVELOPE_GRAMMAR_VERSION: u32 = 2;

const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum FrameKind {
    Tab,
    Report,
    System,
    Reminder,
    Mesh,
    Telegram,
    Pebble,
    Plugin,
    PluginConfig,
    SystemPrompt,
    Voice,
    Neighbor,
}

impl FrameKind {
    pub const ALL: [Self; 12] = [
        Self::Tab,
        Self::Report,
        Self::System,
        Self::Reminder,
        Self::Mesh,
        Self::Telegram,
        Self::Pebble,
        Self::Plugin,
        Self::PluginConfig,
        Self::SystemPrompt,
        Self::Voice,
        Self::Neighbor,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tab => "tab",
            Self::Report => "report",
            Self::System => "system",
            Self::Reminder => "reminder",
            Self::Mesh => "mesh",
            Self::Telegram => "telegram",
            Self::Pebble => "pebble",
            Self::Plugin => "plugin",
            Self::PluginConfig => "plugin_config",
            Self::SystemPrompt => "system_prompt",
            Self::Voice => "voice",
            Self::Neighbor => "neighbor",
        }
    }

    pub fn from_wire(from: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageSegment {
    Text {
        text: String,
    },
    Frame {
        kind: FrameKind,
        from: String,
        attrs: BTreeMap<String, String>,
        body: String,
        tip: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        segments: Vec<MessageSegment>,
    },
    /// A complete frame whose sender kind is not registered or whose opening
    /// tag does not parse; `body` is its markup-free display text.
    UnknownFrame {
        raw: String,
        attrs: BTreeMap<String, String>,
        body: String,
    },
    /// An opening tag whose close has not arrived (streaming or truncation).
    PartialFrame {
        raw: String,
        attrs: BTreeMap<String, String>,
        body: String,
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

impl MessageSegment {
    fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }
}

/// A segment together with the exact input bytes it stands for. The spans of
/// one parse tile the input, which is what makes the grammar total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedSegment {
    pub span: Range<usize>,
    pub segment: MessageSegment,
}

impl SpannedSegment {
    /// Folding host bytes into a span keeps a carried `raw` equal to that span.
    fn resize(&mut self, text: &str, span: Range<usize>) {
        if let MessageSegment::UnknownFrame { raw, .. } | MessageSegment::PartialFrame { raw, .. } =
            &mut self.segment
        {
            *raw = text[span.clone()].to_string();
        }
        self.span = span;
    }
}

pub fn parse_envelope(text: &str) -> Vec<MessageSegment> {
    parse_envelope_spans(text)
        .into_iter()
        .map(|spanned| spanned.segment)
        .collect()
}

pub fn parse_envelope_spans(text: &str) -> Vec<SpannedSegment> {
    parse_nested(text, 0, true)
}

struct Segments {
    spans: Vec<SpannedSegment>,
    run_start: usize,
}

impl Segments {
    fn flush_text(&mut self, text: &str, end: usize) {
        if self.run_start < end {
            self.spans.push(SpannedSegment {
                span: self.run_start..end,
                segment: MessageSegment::Text {
                    text: text[self.run_start..end].to_string(),
                },
            });
        }
        self.run_start = end;
    }

    fn push(&mut self, text: &str, span: Range<usize>, segment: MessageSegment) {
        self.flush_text(text, span.start);
        self.run_start = span.end;
        self.spans.push(SpannedSegment { span, segment });
    }

    fn extend(&mut self, text: &str, start: usize, inner: Vec<SpannedSegment>) {
        self.flush_text(text, start);
        if let Some(last) = inner.last() {
            self.run_start = last.span.end;
        }
        self.spans.extend(inner);
    }

    /// The host types the marker after content it pasted: it closes that run
    /// as a paste, or folds into the segment it trails.
    fn pasted_marker(&mut self, text: &str, span: Range<usize>) {
        let run = &text[self.run_start..span.start];
        if run.trim().is_empty() {
            if let Some(last) = self.spans.last_mut() {
                last.resize(text, last.span.start..span.end);
                self.run_start = span.end;
                return;
            }
        }
        let body = if run.trim().is_empty() {
            ""
        } else {
            strip_line_break(run)
        };
        let start = self.run_start;
        self.run_start = span.end;
        self.spans.push(SpannedSegment {
            span: start..span.end,
            segment: MessageSegment::Paste {
                id: None,
                body: body.to_string(),
            },
        });
    }
}

fn strip_line_break(text: &str) -> &str {
    text.strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text)
}

pub(crate) fn parse_nested(text: &str, depth: usize, allow_frames: bool) -> Vec<SpannedSegment> {
    let mut out = Segments {
        spans: Vec::new(),
        run_start: 0,
    };
    if depth >= MAX_DEPTH {
        out.flush_text(text, text.len());
        return out.spans;
    }
    let mut offset = 0;
    while offset < text.len() {
        if let Some(end) = quoted_end(text, offset) {
            offset = end;
            continue;
        }
        let line_start = offset == 0 || text.as_bytes()[offset - 1] == b'\n';
        let boundary = line_start || offset == out.run_start;
        let rest = &text[offset..];
        if line_start && rest.starts_with(crate::message_markers::OPEN) {
            if let Some(segment) = non_routing_marker(rest, allow_frames) {
                out.push(text, offset..text.len(), segment);
                offset = text.len();
                continue;
            }
        }
        if let Some(length) = pasted_marker_len(rest, line_start) {
            out.pasted_marker(text, offset..offset + length);
            offset += length;
            continue;
        }
        if let Some((segment, consumed)) = attachment(rest) {
            out.push(text, offset..offset + consumed, segment);
            offset += consumed;
            continue;
        }
        if allow_frames && opens_frame(rest) {
            let (segment, consumed) = match frame_extent(rest) {
                Some(consumed) => (frame_segment(&rest[..consumed], depth), consumed),
                None => (partial_frame(rest), rest.len()),
            };
            out.push(text, offset..offset + consumed, segment);
            offset += consumed;
            continue;
        }
        if boundary {
            if let Some(found) = paste(rest) {
                let inner = paste_segments(
                    text,
                    offset,
                    offset + found.consumed,
                    found.id,
                    offset + found.body.start..offset + found.body.end,
                    depth,
                    allow_frames,
                );
                out.extend(text, offset, inner);
                offset += found.consumed;
                continue;
            }
        }
        offset += rest.chars().next().map_or(1, char::len_utf8);
    }
    out.flush_text(text, text.len());
    out.spans
}

/// Paste content is never plain text: text inside the wrapper stays a paste,
/// and whitespace between nested segments folds into its neighbours.
fn paste_segments(
    text: &str,
    start: usize,
    end: usize,
    id: Option<String>,
    body: Range<usize>,
    depth: usize,
    allow_frames: bool,
) -> Vec<SpannedSegment> {
    let inner = parse_nested(&text[body.clone()], depth + 1, allow_frames);
    let structured = inner.iter().any(|spanned| {
        !matches!(
            spanned.segment,
            MessageSegment::Text { .. } | MessageSegment::Paste { .. }
        )
    });
    if !structured {
        let body = inner
            .into_iter()
            .filter_map(|spanned| match spanned.segment {
                MessageSegment::Text { text } => Some(text),
                MessageSegment::Paste { body, .. } => Some(body),
                _ => None,
            })
            .collect::<String>();
        return vec![SpannedSegment {
            span: start..end,
            segment: MessageSegment::Paste { id, body },
        }];
    }
    let mut spans = inner
        .into_iter()
        .filter_map(|spanned| {
            let segment = match spanned.segment {
                MessageSegment::Text { text } | MessageSegment::Paste { body: text, .. }
                    if text.trim().is_empty() =>
                {
                    return None;
                }
                MessageSegment::Text { text } | MessageSegment::Paste { body: text, .. } => {
                    MessageSegment::Paste {
                        id: id.clone(),
                        body: text,
                    }
                }
                segment => segment,
            };
            Some(SpannedSegment {
                span: body.start + spanned.span.start..body.start + spanned.span.end,
                segment,
            })
        })
        .collect::<Vec<_>>();
    let mut cursor = start;
    let count = spans.len();
    for (index, spanned) in spans.iter_mut().enumerate() {
        let span_end = if index + 1 == count {
            end
        } else {
            spanned.span.end
        };
        spanned.resize(text, cursor..span_end);
        cursor = span_end;
    }
    spans
}

fn pasted_marker_len(rest: &str, line_start: bool) -> Option<usize> {
    let tail = rest.strip_prefix(DOMIOS_PASTED_MARKER)?;
    let after = tail.trim_start_matches([' ', '\t']);
    let marker = rest.len() - after.len();
    if after.is_empty() {
        return line_start.then_some(rest.len());
    }
    if let Some(remaining) = after
        .strip_prefix("\r\n")
        .or_else(|| after.strip_prefix('\n'))
    {
        return line_start.then_some(rest.len() - remaining.len());
    }
    (after.starts_with("<domios") && opens_frame(after)).then_some(marker)
}

fn non_routing_marker(rest: &str, allow_frames: bool) -> Option<MessageSegment> {
    let marker = crate::message_markers::parse(rest)?;
    match marker.kind.as_str() {
        "photo" | "file" if marker.attr("file").is_some() => {
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
            let kind = FrameKind::from_wire(&marker.kind)?;
            Some(MessageSegment::Frame {
                kind,
                from: marker.kind,
                attrs: marker.attrs.into_iter().collect(),
                body: marker.payload,
                tip: None,
                segments: Vec::new(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
