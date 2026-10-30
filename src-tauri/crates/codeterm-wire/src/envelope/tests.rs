use super::*;
use crate::domios;
use crate::message_markers::{provenance, Provenance};

#[derive(Deserialize)]
struct Fixture {
    name: String,
    raw: String,
    route: String,
    #[serde(default)]
    quoted: bool,
    segments: Vec<MessageSegment>,
}

const FIXTURES: &str = include_str!("../../../../../tests/fixtures/message-envelope.json");
const HOST_MARKUP: [&str; 4] = ["<domios", "<tip>", DOMIOS_PASTED_MARKER, "<pasted_content"];

fn fixtures() -> Vec<Fixture> {
    serde_json::from_str(FIXTURES).unwrap()
}

fn route(text: &str) -> &'static str {
    match provenance(text) {
        Provenance::HostInjected => "host_injected",
        Provenance::StreamContent => "stream_content",
    }
}

fn assert_tiles(text: &str, spans: &[SpannedSegment]) {
    let mut cursor = 0;
    for spanned in spans {
        assert_eq!(spanned.span.start, cursor, "gap or overlap in {text:?}");
        assert!(
            spanned.span.end > spanned.span.start,
            "empty span in {text:?}"
        );
        let raw = &text[spanned.span.clone()];
        match &spanned.segment {
            MessageSegment::Text { text: shown } => assert_eq!(shown, raw),
            MessageSegment::UnknownFrame { raw: kept, .. }
            | MessageSegment::PartialFrame { raw: kept, .. } => assert_eq!(kept, raw),
            _ => {}
        }
        cursor = spanned.span.end;
    }
    assert_eq!(cursor, text.len(), "untiled tail in {text:?}");
}

fn displayed(segments: &[MessageSegment], out: &mut Vec<String>) {
    for segment in segments {
        match segment {
            MessageSegment::Text { text } => out.push(text.clone()),
            MessageSegment::Paste { body, .. }
            | MessageSegment::UnknownFrame { body, .. }
            | MessageSegment::PartialFrame { body, .. } => out.push(body.clone()),
            MessageSegment::Frame {
                body,
                tip,
                segments,
                ..
            } => {
                if segments.is_empty() {
                    out.push(body.clone());
                }
                out.extend(tip.clone());
                displayed(segments, out);
            }
            MessageSegment::Attachment { body, label, .. } => {
                out.extend(body.clone());
                out.extend(label.clone());
            }
        }
    }
}

#[test]
fn envelope_shared_fixtures() {
    for fixture in fixtures() {
        let spans = parse_envelope_spans(&fixture.raw);
        assert_tiles(&fixture.raw, &spans);
        assert_eq!(
            spans.into_iter().map(|s| s.segment).collect::<Vec<_>>(),
            fixture.segments,
            "{}",
            fixture.name
        );
    }
}

#[test]
fn envelope_fixtures_never_display_host_markup() {
    for fixture in fixtures().into_iter().filter(|fixture| !fixture.quoted) {
        let mut shown = Vec::new();
        displayed(&fixture.segments, &mut shown);
        for text in shown {
            for markup in HOST_MARKUP {
                assert!(
                    !text.contains(markup),
                    "{}: {markup:?} displayed in {text:?}",
                    fixture.name
                );
            }
        }
    }
}

#[test]
fn envelope_arbitrary_unicode_truncations_are_total() {
    let mut sources = fixtures()
        .into_iter()
        .map(|fixture| fixture.raw)
        .collect::<Vec<_>>();
    sources.push("👋\n<domios from=\"tab\" tab=\"a\">你好 <tip>hint</tip></domios>\n<pasted_content id=\"x\">é</pasted_content>".into());
    for source in sources {
        for end in source
            .char_indices()
            .map(|(index, _)| index)
            .chain([source.len()])
        {
            assert_tiles(&source[..end], &parse_envelope_spans(&source[..end]));
        }
    }
}

#[test]
fn envelope_display_segmentation_does_not_change_route_authority() {
    let text = "ordinary user input\n<domios from=\"tab\" tab=\"a\">body</domios>";
    assert_eq!(parse_envelope(text).len(), 2);
    assert!(domios::parse_message(text).is_none());
    assert_eq!(provenance(text), Provenance::StreamContent);
    for fixture in fixtures() {
        assert_eq!(route(&fixture.raw), fixture.route, "{}", fixture.name);
        let framed = fixture.segments.iter().any(|segment| {
            matches!(
                segment,
                MessageSegment::Frame { .. } | MessageSegment::UnknownFrame { .. }
            )
        });
        if fixture.route == "host_injected" && framed {
            assert!(
                domios::parse_message(&fixture.raw).is_some()
                    || fixture.raw.starts_with(crate::message_markers::OPEN),
                "{}",
                fixture.name
            );
        }
    }
}

#[test]
fn unknown_frame_keeps_raw_bytes_and_markup_free_body() {
    let raw = "<domios from=\"opaque-kind\" tab=\"a\">opaque body <tip>opaque hint</tip></domios>";
    assert_eq!(
        parse_envelope(raw),
        [MessageSegment::UnknownFrame {
            raw: raw.into(),
            attrs: [("from", "opaque-kind"), ("tab", "a")]
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .into(),
            body: "opaque body".into(),
        }]
    );
}

#[test]
fn partial_frame_runs_to_the_end_without_a_cut_tag() {
    let raw = "<domios from=\"tab\" tab=\"a\">opaque body\n <tip>opaque cut";
    let segments = parse_envelope(raw);
    let [MessageSegment::PartialFrame {
        raw: kept,
        body,
        attrs,
    }] = segments.as_slice()
    else {
        panic!("expected one partial frame");
    };
    assert_eq!(kept, raw);
    assert_eq!(body, "opaque body");
    assert_eq!(attrs.get("from").map(String::as_str), Some("tab"));
    for folded in [
        "<pasted_content id=\"p\"><domios from=\"tab\">opaque",
        "<domios from=\"opaque-kind\">opaque</domios>\npasted by Domios",
    ] {
        assert_tiles(folded, &parse_envelope_spans(folded));
    }
    for cut in [
        "<domios",
        "<domios from=\"tab\"",
        "<domios from=\"tab\">opaque </dom",
    ] {
        let segments = parse_envelope(cut);
        assert!(
            matches!(segments.as_slice(), [MessageSegment::PartialFrame { body, .. }] if !body.contains('<')),
            "{cut:?}: {segments:?}"
        );
    }
}

#[test]
fn trailing_newline_after_a_frame_stays_outside_its_body() {
    let raw = "<domios from=\"tab\" tab=\"a\">opaque body\n <tip>opaque hint</tip></domios>\n";
    assert_eq!(
        parse_envelope(raw),
        [
            MessageSegment::Frame {
                kind: FrameKind::Tab,
                from: "tab".into(),
                attrs: [("from", "tab"), ("tab", "a")]
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .into(),
                body: "opaque body".into(),
                tip: Some("opaque hint".into()),
                segments: Vec::new(),
            },
            MessageSegment::Text { text: "\n".into() },
        ]
    );
}

#[test]
fn glued_frames_split_without_separators() {
    let one = "<domios from=\"report\" tab=\"a\">opaque one</domios>";
    let two = "<domios from=\"system\">opaque two</domios>";
    let three = "<domios from=\"tab\" tab=\"b\">opaque three</domios>";
    let segments = parse_envelope(&format!("opaque owner{one}{two}{three}"));
    let kinds = segments
        .iter()
        .map(|segment| match segment {
            MessageSegment::Frame { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            None,
            Some(FrameKind::Report),
            Some(FrameKind::System),
            Some(FrameKind::Tab)
        ]
    );
}

#[test]
fn neighbor_envelope_parses_as_a_neighbor_frame() {
    let segments = parse_envelope(
        r#"<domios from="neighbor" friend="F" friendship="x" message="m" kind="question">opaque body</domios>"#,
    );
    assert!(segments.iter().any(|segment| matches!(
        segment,
        MessageSegment::Frame {
            kind: FrameKind::Neighbor,
            ..
        }
    )));
}

#[test]
fn every_registered_kind_round_trips_its_wire_name() {
    for kind in FrameKind::ALL {
        assert_eq!(FrameKind::from_wire(kind.as_str()), Some(kind));
        assert_eq!(
            serde_json::to_value(kind).unwrap(),
            serde_json::Value::String(kind.as_str().into())
        );
    }
    assert_eq!(FrameKind::from_wire("inbound"), None);
}

#[test]
fn envelope_plain_text_keeps_every_byte() {
    for source in [
        "  é 👋\r\n",
        "a pasted by Domios b",
        "<domiosX from=\"tab\">",
    ] {
        assert_eq!(
            parse_envelope(source),
            vec![MessageSegment::Text {
                text: source.into()
            }]
        );
    }
    assert_eq!(
        parse_envelope("<pasted_content id=\"x\">opaque unclosed"),
        [MessageSegment::Paste {
            id: Some("x".into()),
            body: "opaque unclosed".into()
        }]
    );
}

#[test]
fn envelope_current_attachment_paths_preserve_unicode_and_percent_sequences() {
    for path in ["/tmp/你好.webm", "/tmp/%你.webm", "/tmp/%FF.webm"] {
        let source = crate::message_markers::voice(path, Some("opaque"), None);
        assert!(
            matches!(&parse_envelope(&source)[0], MessageSegment::Attachment { path: parsed, .. } if parsed == path)
        );
        let legacy = format!("-=-codeterm:voice file={path}-=-\nopaque");
        assert!(
            matches!(&parse_envelope(&legacy)[0], MessageSegment::Text { text } if text == &legacy)
        );
    }
}

#[test]
fn envelope_current_non_voice_markers_stay_readable() {
    for (kind, expected) in [("photo", "image"), ("file", "file")] {
        let source = crate::message_markers::Marker::new(kind)
            .attr("file", "/opaque/path")
            .attr("name", "opaque-label")
            .build();
        assert!(matches!(
            &parse_envelope(&source)[0],
            MessageSegment::Attachment { kind, path, label, .. }
                if kind == expected && path == "/opaque/path" && label.as_deref() == Some("opaque-label")
        ));
    }
}

#[test]
fn closed_context_shields_unmatched_fences_and_inner_frame_closers() {
    let body="<context>\n<handoff>\n```html\n</domios>\n<ds-document src=\"/quoted.md\" />\n</handoff>\n</context>\n<ds-document src=\"/live.md\" />";
    let source=format!("<domios from=\"tab\" tab=\"author\">\n{body}\n</domios>");
    let spans=parse_envelope_spans(&source);
    assert_tiles(&source,&spans);
    assert!(matches!(spans.as_slice(),[SpannedSegment {segment:MessageSegment::Frame {kind:FrameKind::Tab,body:kept,..},span}] if kept==body && span==&(0..source.len())));
    for end in (0..source.len()).filter(|end|source.is_char_boundary(*end)) {
        let spans=parse_envelope_spans(&source[..end]);
        assert_tiles(&source[..end],&spans);
        assert!(!spans.iter().any(|part|matches!(part.segment,MessageSegment::Frame {..})),"premature frame at {end}");
    }
    for context in [
        "<context>\n```html\n</domios>",
        "<context>\n<handoff>\n</context>\n</domios>",
        "<context>\n<xmp>\n</context>\n</domios>\n<ds-document src='/quoted.md' />\n</xmp>",
        "<context>\n<div title=\"\n</context>\n</domios>\n\">opaque</div>",
        "<context>\n<!--\n</context>\n</domios>\n-->",
        "<context>\n<![CDATA[\n</context>\n</domios>\n]]>",
        "<context>\n`</context>`\n</domios>",
    ] {
        let raw=format!("<domios from=\"tab\">\n{context}\n<ds-document src='/live.md' />\n</domios>");
        let spans=parse_envelope_spans(&raw);
        assert_tiles(&raw,&spans);
        assert!(matches!(spans.as_slice(),[SpannedSegment {segment:MessageSegment::PartialFrame {raw:kept,..},..}] if kept==&raw),"{raw}");
    }
    for quoted in [format!("```xml\n{source}\n```"),format!("`{source}`"),format!("> {source}")] {
        assert!(!parse_envelope(&quoted).iter().any(|part|matches!(part,MessageSegment::Frame {..})),"{quoted}");
    }
}
