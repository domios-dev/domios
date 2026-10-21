use super::*;
use codeterm_wire::envelope::MessageSegment;

#[test]
fn envelope_segments_survive_the_chat_wire_round_trip() {
    let content = "owner\n<domios from=\"tab\" tab=\"a\">routed</domios>";
    let row = ChatMsg {
        kind: "user".into(),
        content: content.into(),
        ..Default::default()
    }
    .with_segments();
    assert!(matches!(
        row.segments.as_deref(),
        Some([MessageSegment::Text { .. }, MessageSegment::Frame { .. }])
    ));
    let wire = serde_json::to_value(&row).unwrap();
    assert_eq!(
        wire["segmentsVersion"],
        codeterm_wire::envelope::ENVELOPE_GRAMMAR_VERSION
    );
    let restored: ChatMsg = serde_json::from_value(wire).unwrap();
    assert_eq!(restored, row);
    assert_eq!(restored.content, content);
}

#[test]
fn envelope_segments_from_wire_keep_the_clean_content() {
    let mut row = ChatMsg {
        kind: "user".into(),
        content: "opaque body".into(),
        ..Default::default()
    };
    row.refresh_segments_from("<domios from=\"tab\" tab=\"a\">opaque body</domios>\n");
    assert_eq!(row.content, "opaque body");
    assert!(matches!(
        row.segments.as_deref(),
        Some([MessageSegment::Frame { .. }, MessageSegment::Text { .. }])
    ));
    assert_eq!(
        row.segments_version,
        Some(codeterm_wire::envelope::ENVELOPE_GRAMMAR_VERSION)
    );
}

#[test]
fn system_send_carries_versioned_segments_on_the_wire() {
    let send = SystemSend::new("<domios from=\"system\">opaque</domios>", 7);
    let wire = serde_json::to_value(&send).unwrap();
    assert_eq!(wire["segments"][0]["type"], "frame");
    assert_eq!(wire["segments"][0]["kind"], "system");
    assert_eq!(
        wire["segments_version"],
        codeterm_wire::envelope::ENVELOPE_GRAMMAR_VERSION
    );
    assert_eq!(serde_json::from_value::<SystemSend>(wire).unwrap(), send);
}

#[test]
fn envelope_segments_refresh_after_content_changes_and_never_parse_tool_output() {
    let mut row = ChatMsg {
        kind: "user".into(),
        content: "first".into(),
        ..Default::default()
    }
    .with_segments();
    row.content = "second".into();
    row.refresh_segments();
    assert_eq!(
        row.segments,
        Some(vec![MessageSegment::Text {
            text: "second".into()
        }])
    );
    row.kind = "tool_result".into();
    row.content = "<domios from=\"tab\" tab=\"a\">untrusted</domios>".into();
    row.refresh_segments();
    assert!(row.segments.is_none());
    assert!(row.segments_version.is_none());
}
