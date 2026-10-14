//! Legacy non-routing CodeTerm markers for attachments and hook metadata.
//! Routed messages use `crate::domios`.
//!
//! ```text
//! -=-codeterm:<kind> k=v k=v …-=-<payload>
//! ```
//!
//! Routed XML frames are never parsed through this legacy marker grammar.
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
    if crate::domios::parse_message(text).is_some() {
        return Provenance::HostInjected;
    }
    let head = text.strip_prefix(BRACKETED_PASTE_START).unwrap_or(text);
    if head.starts_with(OPEN)
        && parse(text).is_some_and(|marker| !is_legacy_route_kind(&marker.kind))
    {
        Provenance::HostInjected
    } else {
        Provenance::StreamContent
    }
}

/// Parse a marker only when it is host-injected (the leading frame). Embedded or
/// whitespace-padded markers return `None` even where [`parse`] alone would
/// accept them. Trust routing metadata through this, never bare [`parse`].
pub fn parse_host_injected(text: &str) -> Option<ParsedMarker> {
    if let Some(message) = crate::domios::parse_message(text) {
        let kind = message.attr("from")?.to_string();
        return Some(ParsedMarker {
            kind,
            attrs: message.attrs.into_iter().collect(),
            payload: message.body,
        });
    }
    match provenance(text) {
        Provenance::HostInjected => {
            parse(text).filter(|marker| !is_legacy_route_kind(&marker.kind))
        }
        Provenance::StreamContent => None,
    }
}

fn is_legacy_route_kind(kind: &str) -> bool {
    matches!(
        kind,
        "tab" | "report" | "system" | "reminder" | "mesh" | "telegram" | "pebble" | "inbound"
    )
}

/// Parse a marker. Returns `None` unless the text starts with a CodeTerm marker
/// frame — plain text falls through unchanged. This recognizes the wire shape
/// only; it makes NO authenticity claim (it trims leading whitespace, which a
/// forger can pad). For a trust decision use [`parse_host_injected`].
pub fn parse(text: &str) -> Option<ParsedMarker> {
    if let Some(message) = crate::domios::parse_message(text) {
        return Some(ParsedMarker {
            kind: message.attr("from")?.to_string(),
            attrs: message.attrs.into_iter().collect(),
            payload: message.body,
        });
    }
    let mut normalized = text.trim_start();
    if let Some(stripped) = normalized.strip_prefix(BRACKETED_PASTE_START) {
        normalized = stripped
            .strip_suffix(BRACKETED_PASTE_END)
            .unwrap_or(stripped);
    }
    if let Some(rest) = normalized.strip_prefix("<domios ") {
        let end = rest.find("/>")?;
        let attributes = crate::domios::parse_attributes(rest[..end].trim_end())?;
        if attributes.get("type")?.as_str() != "attachment"
            || attributes.get("kind")?.as_str() != "voice"
            || attributes.contains_key("from")
        {
            return None;
        }
        let mut attrs = HashMap::from([("file".to_string(), attributes.get("path")?.clone())]);
        if let Some(status) = attributes.get("status") {
            attrs.insert("status".to_string(), status.clone());
        }
        let body = &rest[end + 2..];
        let payload = body
            .strip_prefix("\r\n")
            .or_else(|| body.strip_prefix(['\n', '\r']))
            .unwrap_or(body);
        return Some(ParsedMarker {
            kind: "voice".into(),
            attrs,
            payload: payload.into(),
        });
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
    if kind.is_empty() || is_legacy_route_kind(&kind) {
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

pub fn voice(file: &str, transcript: Option<&str>, note: Option<&str>) -> String {
    let mut attrs = vec![
        ("type", "attachment"),
        ("kind", "voice"),
        ("path", file),
        ("label", "voice"),
    ];
    if transcript.is_none() && note.is_some() {
        attrs.push(("status", "untranscribed"));
    }
    let tag = crate::domios::build_opening_tag(&attrs);
    let mut wire = format!("{} />", tag.trim_end_matches('>'));
    if let Some(payload) = transcript.or(note).filter(|payload| !payload.is_empty()) {
        wire.push('\n');
        wire.push_str(payload);
    }
    wire
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
    telegram_routed(user, msg, None, None, body)
}

/// A Telegram inbound that names its forum thread and, when it reached the
/// General Agent from somewhere else, why.
pub fn telegram_routed(
    user: &str,
    msg: Option<&str>,
    thread: Option<&str>,
    origin: Option<&str>,
    body: &str,
) -> String {
    let mut attrs = vec![("from", "telegram")];
    attrs.push(("user", user));
    if let Some(msg) = msg.filter(|value| !value.is_empty()) {
        attrs.push(("msg", msg));
    }
    if let Some(thread) = thread.filter(|value| !value.is_empty()) {
        attrs.push(("thread", thread));
    }
    if let Some(origin) = origin.filter(|value| !value.is_empty()) {
        attrs.push(("origin", origin));
    }
    crate::domios::DomiosFrame {
        opening_tag: crate::domios::build_opening_tag(&attrs),
        body: body.to_string(),
        suffix: String::new(),
    }
    .to_message()
}

pub fn mesh(peer: &str, tab: &str, body: &str) -> String {
    routed_message("mesh", &[("peer", peer), ("tab", tab)], body)
}

pub fn tab(label: &str, tab: &str, body: &str) -> String {
    routed_message("tab", &[("label", label), ("tab", tab)], body)
}

pub fn report(from: &str, body: &str) -> String {
    routed_message("report", &[("label", from)], body)
}

fn routed_message(kind: &str, attrs: &[(&str, &str)], body: &str) -> String {
    let mut all_attrs = vec![("from", kind)];
    all_attrs.extend_from_slice(attrs);
    crate::domios::DomiosFrame {
        opening_tag: crate::domios::build_opening_tag(&all_attrs),
        body: body.to_string(),
        suffix: String::new(),
    }
    .to_message()
}

pub fn hook(json: &str) -> String {
    Marker::new("hook").payload(json).build()
}

pub fn system_prompt(body: &str) -> String {
    Marker::new("system_prompt").payload(body).build()
}

#[cfg(test)]
mod tests;
