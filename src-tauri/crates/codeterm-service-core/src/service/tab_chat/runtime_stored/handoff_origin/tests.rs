use super::*;
use crate::service::{messenger::Messenger, provider_handoff::input::system_message};

#[test]
fn archived_handoff_restores_recorded_origin_only_for_the_exact_runtime_item() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("handoff.json");
    let body = "opaque handoff payload";
    let wire = system_message(body, Some("Codex"), Some("GPT-6.2-Sol")).unwrap();
    let frame = Messenger::restore_framed_delivery(&wire).unwrap();
    let mut delivered = frame.chat_message("host-send".into()).unwrap();
    assert!(HandoffOrigin::recorded(&delivered).is_none());
    delivered.runtime_source_item_id = Some("provider-item".into());
    HandoffOrigin::recorded(&delivered)
        .unwrap()
        .write(&path)
        .unwrap();
    let mut rows = ["provider-item", "copied-by-user"].map(|id| ChatMsg {
        id: id.into(),
        kind: "user".into(),
        content: wire.clone(),
        ..Default::default()
    });
    assert!(HandoffOrigin::recorded(&rows[0]).is_none());
    HandoffOrigin::read(&path).unwrap().apply(&mut rows);
    assert_eq!(rows[0].content, body);
    assert_eq!(rows[0].inbound_kind.as_deref(), Some("system"));
    assert_eq!(rows[0].from_context.as_deref(), Some(HANDOFF_CONTEXT));
    assert_eq!(rows[0].from_label.as_deref(), Some("Codex"));
    assert_eq!(rows[0].from_model.as_deref(), Some("GPT-6.2-Sol"));
    assert_eq!(rows[1].content, wire);
    assert!(rows[1].inbound_kind.is_none());
}
