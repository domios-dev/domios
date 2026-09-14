//! Unified CodeTerm message markers.
//!
//! One collision-resistant, namespaced envelope for every marker CodeTerm
//! injects into chat messages (attachments, source prefixes, hook metadata):
//!
//! ```text
//! -=-codeterm:<kind> k=v k=v …-=-<payload>
//! ```
//!
//! The `codeterm:` sentinel makes accidental reproduction unlikely.
//!
//! Attribute values are percent-encoded for the structural characters
//! (`%`, space, `=`, newline, tab, `"`) so payloads/labels never break the
//! frame; everything else (notably `/`, `.`, `-` in file paths) stays readable.
//! The TS twin is `packages/chat-core/src/markers.ts` — keep the parity fixture
//! (`tests::PARITY`) in sync with its `markers.test.ts`.

use std::collections::HashMap;

pub const OPEN: &str = "-=-codeterm:";
const NEWLINE_CHAR: char = '\n';
pub const CLOSE: &str = "-=-";
const BRACKETED_PASTE_START: &str = "\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";

/// Percent-encode the structural characters that would otherwise break the
/// marker head. Readable chars (incl. `/ . -`) pass through.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '%' => out.push_str("%25"),
            ' ' => out.push_str("%20"),
            '=' => out.push_str("%3D"),
            '\n' => out.push_str("%0A"),
            '\t' => out.push_str("%09"),
            '"' => out.push_str("%22"),
            _ => out.push(ch),
        }
    }
    out
}

fn decode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Builder for a marker.
pub struct Marker {
    kind: String,
    attrs: Vec<(String, String)>,
    payload: String,
}

impl Marker {
    pub fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_string(),
            attrs: Vec::new(),
            payload: String::new(),
        }
    }

    pub fn attr(mut self, key: &str, value: &str) -> Self {
        self.attrs.push((key.to_string(), encode(value)));
        self
    }

    /// Add an attr only when `value` is `Some` and non-empty.
    pub fn attr_opt(self, key: &str, value: Option<&str>) -> Self {
        match value {
            Some(v) if !v.is_empty() => self.attr(key, v),
            _ => self,
        }
    }

    pub fn payload(mut self, payload: &str) -> Self {
        self.payload = payload.to_string();
        self
    }

    pub fn build(&self) -> String {
        let mut head = format!("{}{}", OPEN, self.kind);
        for (k, v) in &self.attrs {
            head.push(' ');
            head.push_str(k);
            head.push('=');
            head.push_str(v);
        }
        head.push_str(CLOSE);
        // The body reads as its own paragraph after the routing header; `parse`
        // strips this newline, so the payload a receiver sees is unchanged.
        if !self.payload.is_empty() {
            head.push(NEWLINE_CHAR);
        }
        head.push_str(&self.payload);
        head
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedMarker {
    pub kind: String,
    pub attrs: HashMap<String, String>,
    pub payload: String,
}

impl ParsedMarker {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(|s| s.as_str())
    }
}

/// Where a candidate marker came from. Without a crypto transport (roadmap), the
/// only host-controlled signal is POSITION: the daemon prepends its marker as the
/// leading frame of the message it delivers. A well-formed marker anywhere else —
/// inside file contents, tool output, a fetched page, or another pane's scrollback
/// the agent reads — is forged content, never authentic routing metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    HostInjected,
    StreamContent,
}

/// Classify a candidate by provenance. `HostInjected` requires the marker to be
/// the exact leading frame, optionally wrapped in the daemon's bracketed-paste
/// envelope. Any preceding byte — including whitespace an attacker could pad to
/// bypass a naive trim — yields `StreamContent`.
pub fn provenance(text: &str) -> Provenance {
    let head = text.strip_prefix(BRACKETED_PASTE_START).unwrap_or(text);
    if head.starts_with(OPEN) && parse(text).is_some() {
        Provenance::HostInjected
    } else {
        Provenance::StreamContent
    }
}

/// Parse a marker only when it is host-injected (the leading frame). Embedded or
/// whitespace-padded markers return `None` even where [`parse`] alone would
/// accept them. Trust routing metadata through this, never bare [`parse`].
pub fn parse_host_injected(text: &str) -> Option<ParsedMarker> {
    match provenance(text) {
        Provenance::HostInjected => parse(text),
        Provenance::StreamContent => None,
    }
}

/// Parse a marker. Returns `None` unless the text starts with a CodeTerm marker
/// frame — plain text falls through unchanged. This recognizes the wire shape
/// only; it makes NO authenticity claim (it trims leading whitespace, which a
/// forger can pad). For a trust decision use [`parse_host_injected`].
pub fn parse(text: &str) -> Option<ParsedMarker> {
    let mut normalized = text.trim_start();
    if let Some(stripped) = normalized.strip_prefix(BRACKETED_PASTE_START) {
        normalized = stripped
            .strip_suffix(BRACKETED_PASTE_END)
            .unwrap_or(stripped);
    }
    let rest = normalized.strip_prefix(OPEN)?;
    let close = rest.find(CLOSE)?;
    let head = &rest[..close];
    let body = &rest[close + CLOSE.len()..];
    // The separator the builder adds. A PTY rewrites it to CR in transit, so
    // accept either form and hand the receiver the payload it was sent.
    let payload = body
        .strip_prefix("\r\n")
        .or_else(|| body.strip_prefix(NEWLINE_CHAR))
        .or_else(|| body.strip_prefix('\r'))
        .unwrap_or(body);

    let mut tokens = head.split(' ');
    let kind = tokens.next()?.to_string();
    if kind.is_empty() {
        return None;
    }
    let mut attrs = HashMap::new();
    for tok in tokens {
        if tok.is_empty() {
            continue;
        }
        if let Some((k, v)) = tok.split_once('=') {
            attrs.insert(k.to_string(), decode(v));
        }
    }
    Some(ParsedMarker {
        kind,
        attrs,
        payload: payload.to_string(),
    })
}

// ── Convenience constructors (one per kind) ──────────────────────────────────

/// Voice marker. The readable text (transcript, or an untranscribed note like
/// "no daemon — see docs") goes in the PAYLOAD so the agent reads it plainly;
/// `status=untranscribed` flags the note case for the UI to style differently.
pub fn voice(file: &str, transcript: Option<&str>, note: Option<&str>) -> String {
    match (transcript, note) {
        (Some(t), _) => Marker::new("voice").attr("file", file).payload(t).build(),
        (None, Some(n)) => Marker::new("voice")
            .attr("file", file)
            .attr("status", "untranscribed")
            .payload(n)
            .build(),
        (None, None) => Marker::new("voice").attr("file", file).build(),
    }
}

/// Configuration request for a plugin, carrying a core-wrapped (safety-enveloped)
/// description. Rendered as a "Configure <plugin>" card in the chat UI.
pub fn plugin_config(id: &str, name: &str, body: &str) -> String {
    Marker::new("plugin_config")
        .attr("plugin", id)
        .attr("name", name)
        .payload(body)
        .build()
}

pub fn photo(file: &str, name: Option<&str>) -> String {
    Marker::new("photo")
        .attr("file", file)
        .attr_opt("name", name)
        .build()
}

pub fn attachment(file: &str, name: Option<&str>) -> String {
    Marker::new("file")
        .attr("file", file)
        .attr_opt("name", name)
        .build()
}

pub fn telegram(user: &str, msg: Option<&str>, body: &str) -> String {
    Marker::new("telegram")
        .attr("user", user)
        .attr_opt("msg", msg)
        .payload(body)
        .build()
}

pub fn mesh(peer: &str, tab: &str, body: &str) -> String {
    Marker::new("mesh")
        .attr("peer", peer)
        .attr("tab", tab)
        .payload(body)
        .build()
}

pub fn tab(label: &str, tab: &str, body: &str) -> String {
    Marker::new("tab")
        .attr("label", label)
        .attr("tab", tab)
        .payload(body)
        .build()
}

pub fn report(from: &str, body: &str) -> String {
    Marker::new("report")
        .attr("from", from)
        .payload(body)
        .build()
}

pub fn hook(json: &str) -> String {
    Marker::new("hook").payload(json).build()
}

pub fn system_prompt(body: &str) -> String {
    Marker::new("system_prompt").payload(body).build()
}

#[cfg(test)]
#[path = "message_markers_tests.rs"]
mod tests;
