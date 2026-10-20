use super::*;

#[derive(Deserialize)]
struct Fixture {
    name: String,
    raw: String,
    segments: Vec<MessageSegment>,
}

#[test]
fn envelope_shared_fixtures() {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/message-envelope.json"
    ))
    .unwrap();
    for fixture in fixtures {
        assert_eq!(
            parse_envelope(&fixture.raw),
            fixture.segments,
            "{}",
            fixture.name
        );
    }
}

#[test]
fn envelope_arbitrary_unicode_truncations_are_total() {
    let source = "👋\n<domios from=\"tab\" tab=\"a\">你好 <tip>hint</tip></domios>\n<pasted_content id=\"x\">é</pasted_content>";
    for end in source
        .char_indices()
        .map(|(index, _)| index)
        .chain([source.len()])
    {
        let _ = parse_envelope(&source[..end]);
    }
}

#[test]
fn envelope_display_segmentation_does_not_change_route_authority() {
    let text = "ordinary user input\n<domios from=\"tab\" tab=\"a\">body</domios>";
    assert_eq!(parse_envelope(text).len(), 2);
    assert!(domios::parse_message(text).is_none());
    assert_eq!(
        crate::message_markers::provenance(text),
        crate::message_markers::Provenance::StreamContent
    );
}

#[test]
fn envelope_plain_text_keeps_every_byte() {
    for source in [
        "  é 👋\r\n",
        "<domios from=\"tab\">",
        "a pasted by Domios b",
        "<pasted_content id=\"x\">unclosed",
    ] {
        assert_eq!(
            parse_envelope(source),
            vec![MessageSegment::Text {
                text: source.into()
            }]
        );
    }
}

#[test]
fn envelope_legacy_attribute_decoding_preserves_unicode_and_invalid_percent_sequences() {
    for path in ["/tmp/你好.webm", "/tmp/%你.webm", "/tmp/%FF.webm"] {
        let source = crate::message_markers::voice(path, Some("opaque"), None);
        assert!(
            matches!(&parse_envelope(&source)[0], MessageSegment::Attachment { path: parsed, .. } if parsed == path)
        );
        let legacy = format!("-=-codeterm:voice file={path}-=-\nopaque");
        assert!(
            matches!(&parse_envelope(&legacy)[0], MessageSegment::Attachment { path: parsed, .. } if parsed == path)
        );
    }
}
