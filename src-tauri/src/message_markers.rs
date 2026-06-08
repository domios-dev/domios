//! Unified CodeTerm message markers.
//!
//! One collision-resistant, namespaced envelope for every marker CodeTerm
//! injects into chat messages (attachments, source prefixes, hook metadata):
//!
//! ```text
//! [[ct:<kind> k=v k=v …]]<payload>
//! ```
//!
//! The `ct:` namespace — not the brackets — is what makes a user reproducing it
//! by accident unlikely. The sentinel lives in [`OPEN`]/[`CLOSE`] constants, so
//! switching to a stricter form (e.g. `⟦ct:…⟧`) later is a one-line change.
//!
//! Attribute values are percent-encoded for the structural characters
//! (`%`, space, `]`, `=`, newline, tab, `"`) so payloads/labels never break the
//! frame; everything else (notably `/`, `.`, `-` in file paths) stays readable.
//! The TS twin is `packages/chat-core/src/markers.ts` — keep the parity fixture
//! (`tests::PARITY`) in sync with its `markers.test.ts`.

use std::collections::HashMap;

pub const NS: &str = "ct";
pub const OPEN: &str = "[[ct:";
pub const CLOSE: &str = "]]";

/// Percent-encode the structural characters that would otherwise break the
/// `[[ct:…]]` frame. Readable chars (incl. `/ . -`) pass through.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '%' => out.push_str("%25"),
            ' ' => out.push_str("%20"),
            ']' => out.push_str("%5D"),
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

/// Parse a marker. Returns `None` unless the text starts with `[[ct:` and has a
/// closing `]]` — plain text falls through unchanged.
pub fn parse(text: &str) -> Option<ParsedMarker> {
    let rest = text.strip_prefix(OPEN)?;
    let close = rest.find(CLOSE)?;
    let head = &rest[..close];
    let payload = &rest[close + CLOSE.len()..];

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

pub fn mesh(peer: &str, pane: &str, body: &str) -> String {
    Marker::new("mesh")
        .attr("peer", peer)
        .attr("pane", pane)
        .payload(body)
        .build()
}

pub fn pane(label: &str, pane: &str, body: &str) -> String {
    Marker::new("pane")
        .attr("label", label)
        .attr("pane", pane)
        .payload(body)
        .build()
}

pub fn report(from: &str, body: &str) -> String {
    Marker::new("report").attr("from", from).payload(body).build()
}

pub fn hook(json: &str) -> String {
    Marker::new("hook").payload(json).build()
}

#[cfg(test)]
#[path = "message_markers_tests.rs"]
mod tests;
