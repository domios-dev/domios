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
    let restored: ChatMsg = serde_json::from_value(serde_json::to_value(&row).unwrap()).unwrap();
    assert_eq!(restored, row);
    assert_eq!(restored.content, content);
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
}
