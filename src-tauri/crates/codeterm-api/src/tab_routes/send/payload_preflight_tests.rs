use super::{route_payload_bytes, route_send_origin, BatchCommand};
use codeterm_service_core::service::delivery_error::{
    preflight_payload_size, SendOrigin, PROGRAMMATIC_SEND_MAX_BYTES,
};

/// The route seam: the preflight decision gates the delivery closure — a
/// refusal must leave the closure uninvoked (zero delivery-path invocation).
fn route_gate(origin: SendOrigin, payload_bytes: usize) -> (bool, bool) {
    let mut delivered = false;
    let passed = preflight_payload_size("53854ac8", origin, payload_bytes)
        .map(|()| delivered = true)
        .is_ok();
    (passed, delivered)
}

#[test]
fn api_route_payload_preflight_refuses_over_bound_programmatic_without_delivery() {
    // Every existing programmatic marker: service push, source tab, mesh peer.
    for (push, src, peer) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let origin = route_send_origin(push, src, peer);
        assert_eq!(origin, SendOrigin::Programmatic);
        let (passed, delivered) = route_gate(origin, PROGRAMMATIC_SEND_MAX_BYTES + 1);
        assert!(!passed);
        assert!(!delivered, "refused send must never reach delivery");
    }
}

#[test]
fn api_route_payload_preflight_refusal_is_the_typed_pre_write_error() {
    let err = preflight_payload_size(
        "53854ac8",
        route_send_origin(false, true, false),
        PROGRAMMATIC_SEND_MAX_BYTES + 1,
    )
    .unwrap_err();
    assert_eq!(err.code(), "payload_too_large");
    assert_eq!(err.tab, "53854ac8");
    assert_eq!(err.payload_bytes, PROGRAMMATIC_SEND_MAX_BYTES + 1);
    assert_eq!(err.max_bytes, PROGRAMMATIC_SEND_MAX_BYTES);
}

#[test]
fn api_route_payload_preflight_passes_at_bound_programmatic_send() {
    let (passed, delivered) = route_gate(
        route_send_origin(false, true, false),
        PROGRAMMATIC_SEND_MAX_BYTES,
    );
    assert!(passed);
    assert!(delivered);
}

#[test]
fn api_route_payload_preflight_exempts_user_origin_over_bound_send() {
    // No marker on the request ⇒ the existing classification is a user turn.
    let origin = route_send_origin(false, false, false);
    assert_eq!(origin, SendOrigin::User);
    let (passed, delivered) = route_gate(origin, PROGRAMMATIC_SEND_MAX_BYTES + 1);
    assert!(passed);
    assert!(delivered);
}

#[test]
fn api_route_payload_preflight_measures_text_and_batch_byte_length() {
    assert_eq!(route_payload_bytes(None, None), 0);
    // Byte length, not char count.
    assert_eq!(route_payload_bytes(Some("абв"), None), 6);
    let batch = [
        BatchCommand {
            text: "12345".into(),
            delay_ms: None,
        },
        BatchCommand {
            text: "678".into(),
            delay_ms: Some(1),
        },
    ];
    assert_eq!(route_payload_bytes(None, Some(&batch)), 8);
    assert_eq!(route_payload_bytes(Some("ab"), Some(&batch)), 10);
}
