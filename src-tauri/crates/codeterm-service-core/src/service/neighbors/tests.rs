use super::*;

#[test]
fn list_includes_local_scopes_and_preserves_backend_outgoing_requests() {
    let mut settings = storage::Settings::default();
    settings.permissions.insert(
        "granted".into(),
        codeterm_neighbors::permissions::Permission {
            allowed: true,
            scope: "Read shared notes".into(),
            allow_tab_requests: false,
            allow_interactive_content: false,
            ..Default::default()
        },
    );
    let mut list = json!({"schema_version":1,"access":"paid",
        "friends":[{"friendship_id":"granted","displayName":"Pat","my_daily_limit":null,"their_daily_limit":7,"sent_today":2},{"friendship_id":"denied"}],
        "incoming":[{"request_id":"in","uid":"peer","created_at":1}],
        "outgoing":[{"id":"out","typed_email":"recipient@example.test","created_at":2,"state":"pending"}]});
    let outgoing = list["outgoing"].clone();
    annotate_list(&mut list, &settings).unwrap();
    assert_eq!(
        list["friends"][0]["permission"]["scope"],
        "Read shared notes"
    );
    assert_eq!(list["friends"][0]["status"], "connected");
    assert!(list["friends"][0]["my_daily_limit"].is_null());
    assert_eq!(list["friends"][0]["their_daily_limit"], 7);
    assert_eq!(list["friends"][0]["sent_today"], 2);
    assert_eq!(list["friends"][0]["notify"], "in_app");
    assert_eq!(list["friends"][1]["permission"]["allowed"], false);
    assert_eq!(list["outgoing"], outgoing);
    assert_eq!(list["incoming"][0]["request_id"], "in");
    list["access"] = json!("locked");
    annotate_list(&mut list, &storage::Settings::default()).unwrap();
    assert_eq!(list["friends"][0]["status"], "paused");
    assert_eq!(list["friends"][0]["permission"]["allowed"], false);
    assert!(annotate_list(&mut json!({}), &settings).is_err());
}

#[test]
fn registry_commands_match_server_callables_without_extra_fields() {
    let cases = [
        (json!({"operation":"list"}), "friendsList", json!({})),
        (json!({"operation":"refresh"}), "friendsList", json!({})),
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
        json!({"operation":"unknown_operation","uid":"other"}),
        json!({"operation":"unknown_operation","trial_end":1700604800}),
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
fn invitation_refusal_explains_the_paid_plan_requirement() {
    assert_eq!(
        neighbor_refusal(&json!({"error":{"details":{"reason":"invite_requires_paid_plan"}}})),
        "invite_requires_paid_plan: Inviting Neighbors requires an active paid plan."
    );
}

#[test]
fn registry_results_require_the_same_account_and_login_generation() {
    let session = |uid: &str, login_generation| codeterm_account::Session {
        login_generation,
        identity: codeterm_account::Identity {
            uid: uid.into(),
            provider: "firebase".into(),
            email: None,
            display_name: None,
            photo_url: None,
        },
        id_token: codeterm_account::Secret::new("fixture-token"),
        expires_at: None,
    };
    let expected = session("own", 1);
    assert!(same_session(Some(&session("own", 1)), &expected));
    assert!(!same_session(Some(&session("foreign", 1)), &expected));
    assert!(!same_session(Some(&session("own", 2)), &expected));
    assert!(!same_session(None, &expected));
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
    use crate::service::messenger::{MessageEnvelope, Messenger, NeighborMessage};
    let friendship = "a".repeat(64);
    let message_id = "b".repeat(32);
    let envelope = MessageEnvelope {
        from_neighbor: Some(friendship.clone()),
        neighbor_friend: Some("opaque-friend".into()),
        neighbor_message: Some(NeighborMessage {
            message_id: message_id.clone(),
            kind: codeterm_neighbors::wire::Kind::Question,
            in_reply_to: None,
        }),
        from_label: "opaque-peer".into(),
        ..Default::default()
    };
    let frame = Messenger::from(envelope).routed_frame("opaque-body");
    let message = codeterm_wire::domios::parse_message(&frame.to_message()).unwrap();
    assert_eq!(message.attr("from"), Some("neighbor"));
    assert_eq!(message.attr("friendship"), Some(friendship.as_str()));
    assert_eq!(message.attr("friend"), Some("opaque-friend"));
    assert_eq!(message.attr("message"), Some(message_id.as_str()));
    assert_eq!(message.attr("kind"), Some("question"));
    assert_eq!(message.body, "opaque-body");
    assert!(frame.suffix.contains(&format!(
        "domios account neighbors send {friendship} --kind answer --in-reply-to {message_id}"
    )));
    assert!(!frame.suffix.contains("--tab"));
    assert!(message.attr("mesh").is_none());
}

#[tokio::test]
async fn fallback_observation_preserves_bindings_history_and_pause_state() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(crate::multiplexer::state::MultiplexerState::new().0);
    let service = &fixture.service;
    let metadata = storage::Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let mut settings = storage::Settings::default();
    settings.bindings.insert(friendship.clone(), "absent-local-tab".into());
    settings.paused.insert(friendship.clone());
    metadata.save_settings(settings.clone()).await.unwrap();
    let mut list = json!({"access":"paid","friends":[],"incoming":[],"outgoing":[]});
    service.prepare_neighbor_list_observation(&metadata, &mut list).await.unwrap();
    let after = metadata.settings().await.unwrap();
    assert_eq!(after.bindings, settings.bindings);
    assert_eq!(after.paused, settings.paused);
    assert!(!metadata.history(&friendship).await.unwrap().ended);
    assert!(fixture.ui.0.lock().is_empty());
}

#[tokio::test]
async fn confirmed_remove_cleans_only_its_friendship_and_preserves_other_bindings() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(crate::multiplexer::state::MultiplexerState::new().0);
    let service = &fixture.service;
    let metadata = storage::Metadata::new(fixture.temp.path().to_owned(), "owner");
    let first = "a".repeat(64);
    let second = "b".repeat(64);
    let mut settings = storage::Settings::default();
    settings.bindings.insert(first.clone(), "absent-a".into());
    settings.bindings.insert(second.clone(), "absent-b".into());
    metadata.save_settings(settings).await.unwrap();
    service.complete_confirmed_neighbor_removal(metadata.clone(), first.clone(), service.neighbor_history_generation()).await.unwrap();
    assert!(metadata.history(&first).await.unwrap().ended);
    assert!(!metadata.history(&second).await.unwrap().ended);
    let settings = metadata.settings().await.unwrap();
    assert!(!settings.bindings.contains_key(&first));
    assert!(settings.bindings.contains_key(&second));
    service.complete_confirmed_neighbor_removal(metadata.clone(), first, service.neighbor_history_generation()).await.unwrap();
    assert!(metadata.settings().await.unwrap().bindings.contains_key(&second));
}
