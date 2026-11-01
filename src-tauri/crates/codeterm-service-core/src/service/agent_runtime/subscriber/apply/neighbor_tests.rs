use super::*;

#[test]
fn neighbor_handling_resolves_envelope_message_id_from_own_composite_correlation() {
    let message = "b".repeat(32);
    assert_eq!(
        neighbor_message_id(&format!(
            "runtime-envelope:neighbor:opaque_sender_with_underscores_{message}"
        )),
        Some(message.as_str())
    );
    assert_eq!(
        neighbor_message_id(&format!("runtime-envelope:neighbor:{message}")),
        Some(message.as_str())
    );
    assert_eq!(neighbor_message_id("runtime-envelope:local:opaque"), None);
    assert_eq!(neighbor_message_id("runtime-envelope:neighbor:"), None);
}
