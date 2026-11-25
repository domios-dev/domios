use super::*;
use codeterm_wire::envelope::MessageSegment;

#[test]
fn host_stamped_rows_never_rederive_provenance_from_display_content() {
    let claimed = "<domios from=\"tab\" tab=\"claimed\">opaque body</domios>";
    let foreign = "<domios from=\"tab\" tab=\"actual\">other body</domios>";
    for wire in ["", foreign, claimed] {
        let mut row = ChatMsg {
            kind: "user".into(),
            content: claimed.into(),
            inbound_kind: Some("tab".into()),
            ..Default::default()
        };
        row.refresh_segments_from(wire);
        let expected = row.clone();
        assert_eq!(row.with_segments(), expected, "{wire}");
    }
    let missing = ChatMsg {
        kind: "user".into(),
        content: claimed.into(),
        inbound_kind: Some("tab".into()),
        ..Default::default()
    };
    assert_eq!(missing.clone().with_segments(), missing);
    let mut clean = ChatMsg {
        kind: "user".into(),
        content: "opaque body".into(),
        inbound_kind: Some("tab".into()),
        ..Default::default()
    };
    clean.refresh_segments_from(claimed);
    assert_eq!(clean.clone().with_segments(), clean);
}

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

#[test]
fn runtime_source_identity_is_optional_and_round_trips_only_when_present() {
    let old = serde_json::json!({"id":"row","type":"user","content":"opaque body"});
    let mut row: ChatMsg = serde_json::from_value(old).unwrap();
    assert!(row.runtime_source_item_id.is_none());
    assert!(serde_json::to_value(&row)
        .unwrap()
        .get("runtimeSourceItemId")
        .is_none());
    row.runtime_source_item_id = Some("native-source".into());
    let stored = serde_json::to_value(&row).unwrap();
    assert_eq!(stored["runtimeSourceItemId"], "native-source");
    assert_eq!(serde_json::from_value::<ChatMsg>(stored).unwrap(), row);
}

#[test]
fn rich_message_skips_unknown_side_channel_variants_without_dropping_the_message() {
    use crate::agent::background_work::{BackgroundWorkKind, BackgroundWorkState};

    let wire = serde_json::json!({
        "role": "assistant",
        "uuid": "opaque-assistant-row",
        "blocks": [{"kind": "tool_use", "data": {"id": "tool-1", "name": "Bash"}}],
        "background_work": [
            {"id": "tool-1", "state": "future_state_variant", "kind": "shell", "label": "opaque"},
            {"id": "bg-live", "state": "running", "kind": "shell", "label": "opaque-live"}
        ],
        "provider_observations": {"permission_mode": "future_mode_variant"},
        "timelineMarker": {"kind": "future_timeline_marker"}
    });
    let row: RichMessage = serde_json::from_value(wire).expect("primary row survives");
    assert_eq!(row.role, "assistant");
    assert_eq!(row.uuid.as_deref(), Some("opaque-assistant-row"));
    assert_eq!(row.blocks.len(), 1);
    assert_eq!(row.blocks[0].kind, "tool_use");
    assert_eq!(row.background_work.len(), 1);
    assert_eq!(row.background_work[0].id, "bg-live");
    assert_eq!(row.background_work[0].state, BackgroundWorkState::Running);
    assert_eq!(row.background_work[0].kind, Some(BackgroundWorkKind::Shell));
    assert!(row.provider_observations.is_none());
    assert!(row.timeline_marker.is_none());

    for invalid in [
        serde_json::json!({"role": "assistant", "background_work": 7}),
        serde_json::json!({"role": "assistant", "background_work": [null]}),
        serde_json::json!({"role": "assistant", "provider_observations": 7}),
        serde_json::json!({"role": "assistant", "timelineMarker": 7}),
    ] {
        assert!(
            serde_json::from_value::<RichMessage>(invalid).is_err(),
            "structural type errors must still be rejected"
        );
    }
}
