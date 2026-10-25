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
