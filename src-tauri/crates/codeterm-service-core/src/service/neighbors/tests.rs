use super::*;

#[test]
fn registry_commands_match_server_callables_without_extra_fields() {
    let cases = [
        (json!({"operation":"list"}), "friendsList", json!({})),
        (
            json!({"operation":"request","email":"friend@example.test"}),
            "friendsRequest",
            json!({"email":"friend@example.test"}),
        ),
        (
            json!({"operation":"respond","request_id":"opaque","accept":true}),
            "friendsRespond",
            json!({"request_id":"opaque","accept":true}),
        ),
        (
            json!({"operation":"remove","friendship_id":"opaque"}),
            "friendsRemove",
            json!({"friendship_id":"opaque"}),
        ),
        (
            json!({"operation":"set_limit","friendship_id":"opaque","per_hour":42}),
            "friendsSetLimit",
            json!({"friendship_id":"opaque","per_hour":42}),
        ),
    ];
    for (input, name, body) in cases {
        let command: NeighborCommand = serde_json::from_value(input).unwrap();
        assert_eq!(command.callable(), (name, body));
    }
    for input in [
        json!({"operation":"friendsSend"}),
        json!({"operation":"list","uid":"other"}),
        json!({"operation":"request","email":"friend@example.test","plan":"pro"}),
    ] {
        assert!(serde_json::from_value::<NeighborCommand>(input).is_err());
    }
}

#[test]
fn server_locked_and_count_refusals_have_upgrade_paths() {
    for reason in ["friends_locked", "neighbor_limit"] {
        assert!(
            neighbor_refusal(&json!({"error":{"details":{"reason":reason}}})).starts_with(reason)
        );
    }
}

#[test]
fn only_the_authenticated_general_session_can_author_permissions() {
    use super::runtime::authoring_allowed;
    assert!(authoring_allowed(true, true, Some("own"), Some("own")));
    assert!(!authoring_allowed(false, true, Some("own"), Some("own")));
    assert!(!authoring_allowed(true, false, Some("own"), Some("own")));
    assert!(!authoring_allowed(true, true, Some("own"), Some("other")));
    assert!(!authoring_allowed(true, true, None, None));
}

#[test]
fn neighbor_frames_preserve_provenance_without_a_local_reply_route() {
    use crate::service::messenger::{MessageEnvelope, Messenger};
    let envelope = MessageEnvelope {
        from_neighbor: Some("opaque-friendship".into()),
        from_label: "opaque-peer".into(),
        ..Default::default()
    };
    let frame = Messenger::from(envelope).routed_frame("opaque-body");
    let message = codeterm_wire::domios::parse_message(&frame.to_message()).unwrap();
    assert_eq!(message.attr("from"), Some("neighbor"));
    assert_eq!(message.attr("friendship"), Some("opaque-friendship"));
    assert_eq!(message.body, "opaque-body");
    assert!(frame.suffix.is_empty());
    assert!(message.attr("mesh").is_none());
}
