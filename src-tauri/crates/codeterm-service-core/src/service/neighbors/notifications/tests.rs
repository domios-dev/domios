use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

fn incoming(id: u64) -> Entry {
    Entry {
        received_by: None,
        flags: Vec::new(),
        flag_callers: Vec::new(),
        direction: "in".into(),
        author: super::super::history::Author::Person("friend".into()),
        sender_uid: "sender".into(),
        body: format!("{}\nprivate second line", "é".repeat(200)),
        at: id,
        message_id: format!("{id:032x}"),
        kind: Kind::Question,
        in_reply_to: None,
        outcome: None,
        reason: None,
        limit: None,
        resets_at: None,
        retry_after_seconds: None,
        submission: None,
        delivery_status: None,
    }
}

#[test]
fn notification_preference_defaults_to_in_app_and_accepts_only_the_three_modes() {
    let mut settings = Settings::default();
    assert_eq!(settings.neighbor_notify("friend"), NeighborNotify::InApp);
    for (wire, mode) in [
        ("off", NeighborNotify::Off),
        ("in_app", NeighborNotify::InApp),
        ("in_app_and_bridges", NeighborNotify::InAppAndBridges),
    ] {
        assert_eq!(
            serde_json::from_value::<NeighborNotify>(json!(wire)).unwrap(),
            mode
        );
        settings.notifications.insert("friend".into(), mode);
        assert_eq!(
            ServiceContext::neighbor_notification_preferences(
                &settings,
                &json!({"friends":[{"friendship_id":"friend"}]})
            )["friend"],
            wire
        );
    }
    assert!(serde_json::from_value::<NeighborNotify>(json!("all")).is_err());
    assert_eq!(short_line("first\nprivate second", 120), "first");
}

#[tokio::test]
async fn agent_rejections_notify_metadata_only_and_owner_sends_stay_silent() {
    use super::super::{delivery::Rejection, history::Author};
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let generation = service.neighbor_history_generation();
    assert!(AgentNotice::for_author(&Author::Person("me".into()), false).is_none());
    let author = Author::Agent {
        agent: "concierge".into(),
        name: "Concierge".into(),
    };
    for error in [
        Error::DailyLimit {
            limit: 7,
            resets_at: 1793145600,
        },
        Error::RateLimited {
            retry_after_seconds: 60,
        },
        Error::AggregateLimited {
            retry_after_seconds: 90,
        },
        Error::Locked,
        Error::RecipientLocked,
        Error::RecipientNotReceiving,
    ] {
        service
            .notify_neighbor_rejection(
                &metadata,
                &friendship,
                AgentNotice::for_author(&author, true).unwrap(),
                Rejection::from_error(error).unwrap(),
                generation,
            )
            .await
            .unwrap();
    }
    let mut settings = metadata.settings().await.unwrap();
    settings
        .notifications
        .insert(friendship.clone(), NeighborNotify::Off);
    metadata.save_settings(settings).await.unwrap();
    service
        .notify_neighbor_rejection(
            &metadata,
            &friendship,
            AgentNotice::for_author(&author, false).unwrap(),
            Rejection::from_error(Error::Locked).unwrap(),
            generation,
        )
        .await
        .unwrap();
    let events = fixture.ui.0.lock();
    let notices: Vec<_> = events
        .iter()
        .filter(|(event, _)| event == "notification-created")
        .map(|(_, payload)| payload)
        .collect();
    assert_eq!(notices.len(), 6);
    for notice in notices {
        assert!(notice["title"]
            .as_str()
            .unwrap()
            .contains("your concierge's reply was not delivered"));
        assert!(notice["body"].is_null());
        assert_eq!(
            notice["actions"][0]["value"],
            format!("domios://neighbor/{friendship}/permissions")
        );
    }
}

#[tokio::test]
async fn arrivals_notify_once_with_a_bounded_preview_and_obey_off_signout_and_outgoing_fences() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    service.remember_neighbor_names(
        &json!({"friends":[{"friendship_id":friendship,"displayName":"Taylor"}]}),
    );
    let generation = service.neighbor_history_generation();
    service
        .append_neighbor_history(&metadata, &friendship, incoming(1), generation)
        .await
        .unwrap();
    service
        .append_neighbor_history(&metadata, &friendship, incoming(1), generation)
        .await
        .unwrap();
    let mut outgoing = incoming(2);
    outgoing.direction = "out".into();
    service
        .append_neighbor_history(&metadata, &friendship, outgoing, generation)
        .await
        .unwrap();
    let mut settings = metadata.settings().await.unwrap();
    settings
        .notifications
        .insert(friendship.clone(), NeighborNotify::Off);
    metadata.save_settings(settings).await.unwrap();
    service
        .append_neighbor_history(&metadata, &friendship, incoming(3), generation)
        .await
        .unwrap();
    service.neighbor_identity_changed();
    assert!(matches!(
        service
            .append_neighbor_history(&metadata, &friendship, incoming(4), generation)
            .await,
        Err(Error::Revoked)
    ));
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    assert!(matches!(
        service
            .append_neighbor_history(
                &metadata,
                &friendship,
                incoming(5),
                service.neighbor_history_generation()
            )
            .await,
        Err(Error::Disabled)
    ));
    let events = fixture.ui.0.lock();
    let arrivals: Vec<_> = events
        .iter()
        .filter(|(event, _)| event == "notification-created")
        .map(|(_, payload)| payload)
        .collect();
    assert_eq!(arrivals.len(), 1);
    let notice = arrivals[0];
    let title = notice["title"].as_str().unwrap();
    assert!(title.starts_with("Taylor asked: "));
    assert!(title.chars().count() < 200);
    assert!(!title.contains("private second line"));
    assert!(notice["body"].is_null());
    assert_eq!(
        notice["actions"][0]["value"],
        format!("domios://neighbor/{friendship}")
    );
}
