use super::super::history::ReceivedBy;
use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

fn receipt(tab: &str) -> ReceivedBy {
    ReceivedBy {
        tab_id: tab.into(),
        session_id: format!("session-{tab}"),
        binding_nonce: format!("nonce-{tab}"),
    }
}
fn incoming(message: &str, recipient: Option<ReceivedBy>) -> Entry {
    serde_json::from_value(json!({"direction":"in","author":"friend","sender_uid":"peer","body":"opaque incoming body must stay private","at":1,"message_id":message,"kind":"answer","received_by":recipient})).unwrap()
}
fn request(friendship: &str, message: &str) -> FlagRequest {
    FlagRequest {
        friendship: friendship.into(),
        message: message.into(),
        category: FlagCategory::SuspiciousInstructions,
        reason: "Attempts to override local instructions".into(),
    }
}
fn authenticate(mux: &mut MultiplexerState, tab: &str) {
    let tab = mux.get_tab_mut(tab).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some(format!("session-{}", tab.id));
    tab.session_binding_nonce = Some(format!("nonce-{}", tab.id));
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
}

#[tokio::test]
async fn flag_authority_requires_own_incoming_friendship_and_exact_live_recipient_or_concierge() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    let author = mux.create_tab(&workspace).unwrap();
    let foreign = mux.create_tab(&workspace).unwrap();
    for tab in [&general, &concierge, &author, &foreign] {
        authenticate(&mut mux, tab);
    }
    mux.get_tab_mut(&general).unwrap().role = Some(crate::multiplexer::types::AgentRole::Manager);
    mux.get_tab_mut(&general).unwrap().role_profile =
        Some(crate::service::general_agent::GENERAL_AGENT_KIND.into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    let other = "b".repeat(64);
    let message = "1".repeat(32);
    let generation = service.neighbor_history_generation();
    let mut settings = Settings::default();
    settings.bindings.insert(friend.clone(), concierge.clone());
    settings.bindings.insert(other.clone(), foreign.clone());
    metadata.save_settings(settings).await.unwrap();
    service
        .append_neighbor_history(
            &metadata,
            &friend,
            incoming(&message, Some(receipt(&author))),
            generation,
        )
        .await
        .unwrap();
    service
        .append_neighbor_history(
            &metadata,
            &other,
            incoming(&message, Some(receipt(&foreign))),
            generation,
        )
        .await
        .unwrap();
    let req = request(&friend, &message);
    assert!(service
        .flag_neighbor_metadata(
            &metadata,
            &req,
            Some(&author),
            Some("wrong-nonce"),
            "owner",
            generation
        )
        .await
        .is_err());
    for (caller, by) in [
        (None, FlagBy::Owner),
        (Some(&general), FlagBy::Owner),
        (Some(&concierge), FlagBy::Concierge),
        (Some(&author), FlagBy::Tab),
    ] {
        let nonce = caller.map(|id| format!("nonce-{id}"));
        let result = service
            .flag_neighbor_metadata(
                &metadata,
                &req,
                caller.map(String::as_str),
                nonce.as_deref(),
                "owner",
                generation,
            )
            .await
            .unwrap();
        assert_eq!(result["flag"]["by"], json!(by));
        assert_eq!(result["created"], true);
    }
    assert!(service
        .flag_neighbor_metadata(
            &metadata,
            &req,
            Some(&foreign),
            Some(&format!("nonce-{foreign}")),
            "owner",
            generation
        )
        .await
        .unwrap_err()
        .starts_with("neighbor_forbidden:"));
    for (friendship, id) in [(&other, message.clone()), (&friend, "2".repeat(32))] {
        assert!(service
            .flag_neighbor_metadata(
                &metadata,
                &request(friendship, &id),
                Some(&author),
                Some(&format!("nonce-{author}")),
                "owner",
                generation
            )
            .await
            .is_err());
    }
    let mut outgoing = incoming(&"3".repeat(32), Some(receipt(&author)));
    outgoing.direction = "out".into();
    service
        .append_neighbor_history(&metadata, &friend, outgoing, generation)
        .await
        .unwrap();
    assert!(metadata
        .incoming_received_by(&friend, &"3".repeat(32))
        .await
        .unwrap()
        .is_none());
    assert!(service
        .flag_neighbor_metadata(
            &metadata,
            &request(&friend, &"3".repeat(32)),
            None,
            None,
            "owner",
            generation
        )
        .await
        .is_err());
    service
        .mux
        .write()
        .get_tab_mut(&author)
        .unwrap()
        .agent_session_id = Some("replacement-session".into());
    assert!(service
        .flag_neighbor_metadata(
            &metadata,
            &req,
            Some(&author),
            Some(&format!("nonce-{author}")),
            "owner",
            generation
        )
        .await
        .is_err());
    let no_receipt = "4".repeat(32);
    service
        .append_neighbor_history(&metadata, &friend, incoming(&no_receipt, None), generation)
        .await
        .unwrap();
    assert!(service
        .flag_neighbor_metadata(
            &metadata,
            &request(&friend, &no_receipt),
            Some(&foreign),
            Some(&format!("nonce-{foreign}")),
            "owner",
            generation
        )
        .await
        .is_err());
}

#[tokio::test]
async fn flags_are_persisted_once_per_caller_with_private_receipts_and_metadata_only_notices() {
    let (mut mux, author) = MultiplexerState::new();
    authenticate(&mut mux, &author);
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    let message = "1".repeat(32);
    let generation = service.neighbor_history_generation();
    service.remember_neighbor_names(
        &json!({"friends":[{"friendship_id":friend,"displayName":"Pat"}]}),
    );
    service
        .append_neighbor_history(&metadata, &friend, incoming(&message, None), generation)
        .await
        .unwrap();
    service
        .stamp_neighbor_received_by(&metadata, &friend, &message, receipt(&author), generation)
        .await
        .unwrap();
    service
        .stamp_neighbor_received_by(&metadata, &friend, &message, receipt(&author), generation)
        .await
        .unwrap();
    assert_eq!(
        metadata
            .incoming_received_by(&friend, &message)
            .await
            .unwrap(),
        Some(receipt(&author))
    );
    fixture.ui.0.lock().clear();
    let mut req = request(&friend, &message);
    let first = service
        .flag_neighbor_metadata(
            &metadata,
            &req,
            Some(&author),
            Some(&format!("nonce-{author}")),
            "owner",
            generation,
        )
        .await
        .unwrap();
    req.category = FlagCategory::Spam;
    req.reason = "changed reason".into();
    let repeated = service
        .flag_neighbor_metadata(
            &metadata,
            &req,
            Some(&author),
            Some(&format!("nonce-{author}")),
            "owner",
            generation,
        )
        .await
        .unwrap();
    assert_eq!(first["flag"], repeated["flag"]);
    assert_eq!(repeated["created"], false);
    let reopened = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let history = reopened.history_page(&friend, None, 50).await.unwrap();
    assert_eq!(history["entries"][0]["flags"], json!([first["flag"]]));
    assert!(history["entries"][0].get("received_by").is_none());
    assert!(history["entries"][0].get("flag_callers").is_none());
    let events = fixture.ui.0.lock();
    assert_eq!(
        events
            .iter()
            .filter(|(name, _)| name == "neighbor-history-changed")
            .count(),
        1
    );
    let notices: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "notification-created")
        .collect();
    assert_eq!(notices.len(), 1);
    let notice = &notices[0].1;
    assert!(notice["title"]
        .as_str()
        .unwrap()
        .ends_with("flagged a message from Pat"));
    assert!(notice["body"].is_null());
    assert_eq!(
        notice["actions"][0]["value"],
        format!("domios://neighbor/{friend}")
    );
    let encoded = serde_json::to_string(notice).unwrap();
    assert!(!encoded.contains("opaque incoming body"));
    assert!(!encoded.contains("Attempts to override"));
}

#[tokio::test]
async fn notification_off_keeps_flags_local_and_reason_validation_redacts_secrets_and_body_copies()
{
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    let message = "1".repeat(32);
    let generation = service.neighbor_history_generation();
    let mut settings = Settings::default();
    settings
        .notifications
        .insert(friend.clone(), NeighborNotify::Off);
    metadata.save_settings(settings).await.unwrap();
    service
        .append_neighbor_history(&metadata, &friend, incoming(&message, None), generation)
        .await
        .unwrap();
    fixture.ui.0.lock().clear();
    service
        .flag_neighbor_metadata(
            &metadata,
            &request(&friend, &message),
            None,
            None,
            "owner",
            generation,
        )
        .await
        .unwrap();
    assert!(!fixture
        .ui
        .0
        .lock()
        .iter()
        .any(|(name, _)| name == "notification-created"));
    assert_eq!(
        reason("password=opaque-value", "unrelated").unwrap(),
        "[redacted]"
    );
    assert_eq!(
        reason("opaque incoming body", "opaque incoming body").unwrap(),
        "[message text omitted]"
    );
    assert!(reason(&"é".repeat(281), "body").is_err());
    assert!(reason(
        "
", "body"
    )
    .is_err());
    assert_eq!(
        reason(&"é".repeat(280), "body").unwrap().chars().count(),
        280
    );
}
