use super::*;
use crate::service::messenger::MessageEnvelope;

#[test]
fn restored_frame_keeps_every_byte_and_survives_serialization() {
    let frame = Messenger::from(MessageEnvelope::codeterm_reminder("build"))
        .framed_delivery("opaque alpha\nopaque beta");
    let original = codeterm_wire::domios::with_pasted_marker(&frame.to_message()).unwrap();
    let restored = Messenger::restore_framed_delivery(&original).unwrap();
    assert_eq!(restored.to_message(), original);
    let bytes = serde_json::to_vec(&restored).unwrap();
    let read: FramedDelivery = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(read.to_message(), original);
    assert!(Messenger::restore_framed_delivery("opaque unframed").is_err());
    assert!(serde_json::from_value::<FramedDelivery>(serde_json::json!({
        "transport": "routed_frame", "opening_tag": "", "body": "opaque", "suffix": ""
    }))
    .is_err());
}

#[test]
fn human_and_bootstrap_inputs_preserve_bytes_without_a_frame() {
    let text = "opaque alpha\nopaque beta";
    for input in [
        AgentInput::Human(HumanInput::composer(text)),
        AgentInput::Launch(LaunchBootstrap::new(text)),
    ] {
        assert_eq!(input.text(), text);
    }
}
