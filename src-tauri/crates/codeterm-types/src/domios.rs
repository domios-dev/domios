use std::collections::BTreeMap;

pub const DOMIOS_CLOSE_TAG: &str = "</domios>";
pub const DOMIOS_LEGACY_TYPED_INSTRUCTION: &str = "Handle the message above.";
pub const DOMIOS_PASTED_MARKER: &str = "pasted by Domios";
pub const LEGACY_CHAT_FOLLOW_UPS: [&str; 2] = [
    " Handle the paste as my request.",
    " Handle the pasted message above as my request.",
];

pub fn typed_pasted_marker() -> String {
    format!(" {DOMIOS_PASTED_MARKER}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomiosFrame {
    pub opening_tag: String,
    pub body: String,
    pub suffix: String,
}

impl DomiosFrame {
    pub fn to_message(&self) -> String {
        format!(
            "{}{}{}{}",
            self.opening_tag, self.body, self.suffix, DOMIOS_CLOSE_TAG
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDomiosMessage {
    pub attrs: BTreeMap<String, String>,
    pub content: String,
    pub body: String,
}

impl ParsedDomiosMessage {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
}

pub fn build_opening_tag(attrs: &[(&str, &str)]) -> String {
    let mut tag = String::from("<domios");
    for (name, value) in attrs {
        tag.push(' ');
        tag.push_str(name);
        tag.push_str("=\"");
        tag.push_str(&escape_attribute(value));
        tag.push('"');
    }
    tag.push('>');
    tag
}

pub fn parse_message(text: &str) -> Option<ParsedDomiosMessage> {
    let rest = text.strip_prefix("<domios")?;
    if !rest.starts_with(' ') && !rest.starts_with('>') {
        return None;
    }
    let opening_end = rest.find('>')?;
    let attrs = parse_attributes(&rest[..opening_end])?;
    if !attrs.get("from").is_some_and(|from| !from.is_empty()) {
        return None;
    }
    let content = &rest[opening_end + 1..];
    let content = content.strip_suffix(DOMIOS_CLOSE_TAG)?;
    Some(ParsedDomiosMessage {
        attrs,
        content: content.to_string(),
        body: strip_typed_suffix(content).to_string(),
    })
}

pub fn with_legacy_typed_instruction(message: &str) -> Option<String> {
    let body = message.strip_suffix(DOMIOS_CLOSE_TAG)?;
    Some(format!(
        "{body} <tip>{DOMIOS_LEGACY_TYPED_INSTRUCTION}</tip>{DOMIOS_CLOSE_TAG}"
    ))
}

pub fn with_pasted_marker(message: &str) -> Option<String> {
    let body = message.strip_suffix(DOMIOS_CLOSE_TAG)?;
    Some(format!("{body}{}{DOMIOS_CLOSE_TAG}", typed_pasted_marker()))
}

pub fn strip_plain_typed_tail(content: &str) -> &str {
    let marker = typed_pasted_marker();
    LEGACY_CHAT_FOLLOW_UPS
        .iter()
        .copied()
        .chain([marker.as_str()])
        .find_map(|tail| content.strip_suffix(tail))
        .unwrap_or(content)
}

pub fn strip_typed_suffix(content: &str) -> &str {
    let mut body = strip_plain_typed_tail(content);
    while let Some(prefix) = body.strip_suffix("</tip>") {
        let Some(start) = prefix.rfind("<tip>") else {
            break;
        };
        body = prefix[..start]
            .strip_suffix(' ')
            .unwrap_or(&prefix[..start]);
    }
    let instruction = format!(" {DOMIOS_LEGACY_TYPED_INSTRUCTION}");
    if let Some(stripped) = body.strip_suffix(&instruction) {
        body = stripped;
    }
    body = strip_plain_typed_tail(body);
    let reply_start = [
        " [to reply, run: codeterm send ",
        " [sender is a shell; reply here]",
    ]
    .into_iter()
    .filter_map(|suffix| body.rfind(suffix))
    .max();
    reply_start.map_or(body, |index| &body[..index])
}

fn escape_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\n' | '\r' | '\t' | '\u{2028}' | '\u{2029}' => escaped.push(' '),
            value if value.is_control() => {}
            value => escaped.push(value),
        }
    }
    escaped
}

fn decode_attribute(value: &str) -> Option<String> {
    let mut decoded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find('&') {
        decoded.push_str(&rest[..index]);
        rest = &rest[index..];
        let (entity, value) = [
            ("&amp;", '&'),
            ("&quot;", '"'),
            ("&lt;", '<'),
            ("&gt;", '>'),
        ]
        .into_iter()
        .find_map(|(entity, value)| rest.strip_prefix(entity).map(|tail| (tail, value)))?;
        decoded.push(value);
        rest = entity;
    }
    decoded.push_str(rest);
    Some(decoded)
}

pub(crate) fn parse_attributes(source: &str) -> Option<BTreeMap<String, String>> {
    let mut attrs = BTreeMap::new();
    let mut rest = source.trim_start();
    while !rest.is_empty() {
        let equals = rest.find('=')?;
        let name = &rest[..equals];
        if name.is_empty()
            || !name.as_bytes()[0].is_ascii_alphabetic()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return None;
        }
        let source = &rest[equals + 1..];
        let (raw, tail) = if let Some(quoted) = source.strip_prefix('"') {
            let end = quoted.find('"')?;
            (&quoted[..end], &quoted[end + 1..])
        } else {
            let end = source.find(char::is_whitespace).unwrap_or(source.len());
            let raw = &source[..end];
            if raw.is_empty() || raw.contains(['"', '\'', '<', '>', '`', '=']) {
                return None;
            }
            (raw, &source[end..])
        };
        let value = decode_attribute(raw)?;
        if attrs.insert(name.to_string(), value).is_some() {
            return None;
        }
        rest = tail;
        if !rest.is_empty()
            && !rest.starts_with(' ')
            && !rest.starts_with('\t')
            && !rest.starts_with('\n')
            && !rest.starts_with('\r')
        {
            return None;
        }
        rest = rest.trim_start();
    }
    Some(attrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_round_trips_escaped_attributes_and_close_tag() {
        let frame = DomiosFrame {
            opening_tag: build_opening_tag(&[("from", "tab"), ("label", "A & \"B\"")]),
            body: "only body".into(),
            suffix: " [reply]".into(),
        };
        let parsed = parse_message(&frame.to_message()).expect("leading frame parses");
        assert_eq!(parsed.attr("from"), Some("tab"));
        assert_eq!(parsed.attr("label"), Some("A & \"B\""));
        assert_eq!(parsed.content, "only body [reply]");
    }

    #[test]
    fn quoted_and_unquoted_leading_frames_project_identically() {
        let quoted =
            parse_message("<domios from=\"system\" context=\"assigned-task\">opaque body</domios>")
                .unwrap();
        let unquoted =
            parse_message("<domios from=system context=assigned-task>opaque body</domios>")
                .unwrap();
        assert_eq!(quoted, unquoted);
        assert_eq!(unquoted.attr("from"), Some("system"));
        assert_eq!(unquoted.body, "opaque body");
        let marker = crate::message_markers::parse_host_injected(
            "<domios from=system context=assigned-task>opaque body</domios>",
        )
        .unwrap();
        assert_eq!(marker.kind, "system");
        assert_eq!(marker.payload, "opaque body");
        for invalid in [
            "<domios from=system context=two words>x</domios>",
            "<domios from=system from=tab>x</domios>",
            "prefix<domios from=system>x</domios>",
        ] {
            assert!(parse_message(invalid).is_none());
        }
    }

    #[test]
    fn opening_tag_attributes_cannot_add_typed_line_breaks() {
        let tag = build_opening_tag(&[("label", "first\nsecond\tthird\rfourth")]);
        assert_eq!(tag, "<domios label=\"first second third fourth\">");
        assert!(!tag.contains('\n'));
        assert!(!tag.contains('\r'));
    }

    #[test]
    fn embedded_and_whitespace_padded_tags_are_content() {
        let embedded = format!("body {}", build_opening_tag(&[("from", "report")]));
        assert!(parse_message(&embedded).is_none());
        assert!(parse_message(&format!(" {embedded}")).is_none());
    }

    #[test]
    fn leading_attachment_tag_is_not_provenance() {
        let path = "C:/Users/me/AppData/Local/Temp/codeterm-chat/a1.png";
        let tag = format!(
            "<domios type=\"attachment\" kind=\"image\" path=\"{path}\" label=\"image1\" />"
        );
        assert!(parse_message(&format!("{tag} what is this")).is_none());
        assert!(parse_message(&format!("{tag} spoof</domios>")).is_none());
    }

    #[test]
    fn malformed_or_nonterminal_close_tags_are_rejected() {
        assert!(parse_message("<domios from=>body</domios>").is_none());
        assert!(parse_message("<domiosX from=\"tab\">body</domios>").is_none());
        assert!(parse_message("<domios>body</domios>").is_none());
        assert!(parse_message("<domios from=\"tab\">body</domios>tail").is_none());
    }

    #[test]
    fn tip_suffix_is_projected_out_but_preserved_in_agent_content() {
        let frame = DomiosFrame {
            opening_tag: build_opening_tag(&[("from", "tab"), ("tab", "abc12345")]),
            body: "opaque body".into(),
            suffix: " <tip>opaque hint</tip> <tip>opaque instruction</tip>".into(),
        };
        let parsed = parse_message(&frame.to_message()).unwrap();
        assert_eq!(parsed.body, frame.body);
        assert_eq!(parsed.content, format!("{}{}", frame.body, frame.suffix));
    }

    #[test]
    fn strip_typed_reply_and_instruction_without_changing_the_body() {
        let body = "opaque routed body";
        let suffix = format!(
            " [to reply, run: codeterm send \"YOUR REPLY\" --tab abc12345] {DOMIOS_LEGACY_TYPED_INSTRUCTION}"
        );
        let frame = DomiosFrame {
            opening_tag: build_opening_tag(&[("from", "tab"), ("tab", "abc12345")]),
            body: body.to_string(),
            suffix,
        };
        let parsed = parse_message(&frame.to_message()).expect("routed frame parses");
        assert_eq!(parsed.body, body);
    }

    #[test]
    fn legacy_typed_form_round_trips_to_the_same_body() {
        let message = "<domios from=\"tab\" tab=\"abc12345\">opaque body [hint]</domios>";
        let legacy = with_legacy_typed_instruction(message).unwrap();
        assert_ne!(legacy, message);
        assert_eq!(
            parse_message(&legacy).unwrap().body,
            parse_message(message).unwrap().body
        );
        assert!(with_legacy_typed_instruction("unframed").is_none());
    }

    #[test]
    fn pasted_marker_and_legacy_tails_are_stripped_from_the_body() {
        let message = "<domios from=\"tab\" tab=\"abc12345\">opaque body</domios>";
        let marked = with_pasted_marker(message).unwrap();
        assert_eq!(parse_message(&marked).unwrap().body, "opaque body");
        assert_eq!(strip_plain_typed_tail("plain pasted by Domios"), "plain");
        for tail in LEGACY_CHAT_FOLLOW_UPS {
            assert_eq!(strip_plain_typed_tail(&format!("plain{tail}")), "plain");
        }
        assert_eq!(strip_plain_typed_tail("plain"), "plain");
    }
}
