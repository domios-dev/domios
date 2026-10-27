use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

fn entry(id: usize, direction: &str, kind: Kind, reply: Option<String>) -> Entry {
    Entry {
        direction: direction.into(),
        author: Author::Person(if direction == "in" { "friend" } else { "me" }.into()),
        sender_uid: "opaque-sender".into(),
        body: format!("opaque-{id}"),
        at: id as u64,
        message_id: format!("{id:032x}"),
        kind,
        in_reply_to: reply,
        outcome: None,
        reason: None,
        limit: None,
        resets_at: None,
        retry_after_seconds: None,
        delivery_status: (direction == "out").then_some(DeliveryStatus::Pending),
    }
}

#[test]
fn bounded_history_pages_newest_first_and_links_answer_and_refusal() {
    let mut history = History::default();
    for id in 0..300 {
        history
            .append(entry(id, "in", Kind::Question, None))
            .unwrap();
    }
    assert_eq!(history.entries.len(), MAX_ENTRIES);
    assert!(!history
        .append(entry(299, "in", Kind::Question, None))
        .unwrap());
    let first = history.page(None, 2);
    assert_eq!(first["entries"][0]["message_id"], format!("{:032x}", 299));
    let second = history.page(first["next_before"].as_str(), 2);
    assert_eq!(second["entries"][0]["message_id"], format!("{:032x}", 297));
    history
        .append(entry(
            300,
            "out",
            Kind::Refusal,
            Some(format!("{:032x}", 299)),
        ))
        .unwrap();
    let incoming = history
        .entries
        .iter()
        .find(|entry| entry.at == 299)
        .unwrap();
    assert_eq!(incoming.outcome.as_deref(), Some("refused"));
    assert_eq!(incoming.reason.as_deref(), Some("opaque-300"));
    history
        .append(entry(
            301,
            "out",
            Kind::Answer,
            Some(format!("{:032x}", 298)),
        ))
        .unwrap();
    assert_eq!(
        history
            .entries
            .iter()
            .find(|entry| entry.at == 298)
            .unwrap()
            .outcome
            .as_deref(),
        Some("answered")
    );
    for id in 400..500 {
        let mut large = entry(id, "in", Kind::Question, None);
        large.body = "\u{0001}".repeat(8176);
        history.append(large).unwrap();
    }
    assert!(serde_json::to_vec(&history).unwrap().len() <= MAX_BYTES);
}

#[tokio::test]
async fn private_history_persists_status_and_end_and_stops_writing_when_disabled() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let outgoing = entry(1, "out", Kind::Question, None);
    service
        .append_neighbor_history(
            &metadata,
            &friendship,
            outgoing.clone(),
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    service
        .append_neighbor_history(
            &metadata,
            &friendship,
            outgoing.clone(),
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    service
        .update_neighbor_history_status(
            &metadata,
            &outgoing.message_id,
            DeliveryStatus::Delivered,
            None,
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    let page = metadata.history_page(&friendship, None, 50).await.unwrap();
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert_eq!(page["entries"][0]["delivery_status"], "delivered");
    let foreign = Metadata::new(fixture.temp.path().to_owned(), "another-owner");
    assert!(
        foreign.history_page(&friendship, None, 50).await.unwrap()["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
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
                entry(2, "in", Kind::Question, None),
                service.neighbor_history_generation()
            )
            .await,
        Err(Error::Disabled)
    ));
    assert_eq!(
        metadata.history_page(&friendship, None, 50).await.unwrap()["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    service
        .end_neighbor_history(
            &metadata,
            &friendship,
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    assert!(
        metadata.history_page(&friendship, None, 50).await.unwrap()["ended"]
            .as_bool()
            .unwrap()
    );
    assert!(matches!(
        service
            .append_neighbor_history(
                &metadata,
                &friendship,
                entry(3, "in", Kind::Question, None),
                service.neighbor_history_generation()
            )
            .await,
        Err(Error::Revoked)
    ));
    let events = fixture.ui.0.lock();
    assert_eq!(
        events
            .iter()
            .filter(|(event, _)| event == "neighbor-history-changed")
            .count(),
        3
    );
    assert!(events
        .iter()
        .all(|(_, payload)| !payload.to_string().contains("opaque")));
}

#[test]
fn history_author_is_owner_or_the_actual_local_agent() {
    let (mut mux, id) = MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().title = "General".into();
    let fixture = ServiceFixture::new(mux);
    assert_eq!(
        fixture.service.neighbor_author(None).unwrap(),
        Author::Person("me".into())
    );
    assert_eq!(
        fixture.service.neighbor_author(Some(&id)).unwrap(),
        Author::Agent {
            agent: id,
            name: "General".into()
        }
    );
}

#[tokio::test]
async fn local_history_retains_daily_limit_details_on_explicit_status_checks() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let outgoing = entry(1, "out", Kind::Question, None);
    service
        .append_neighbor_history(
            &metadata,
            &friendship,
            outgoing.clone(),
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    service
        .update_neighbor_history_status(
            &metadata,
            &outgoing.message_id,
            DeliveryStatus::NotDelivered,
            super::super::delivery::Rejection::from_error(Error::DailyLimit {
                limit: 9,
                resets_at: 1000,
            }),
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    service
        .update_neighbor_history_status(
            &metadata,
            &outgoing.message_id,
            DeliveryStatus::NotDelivered,
            None,
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    let page = metadata.history_page(&friendship, None, 1).await.unwrap();
    assert_eq!(page["entries"][0]["delivery_status"], "not_delivered");
    assert_eq!(page["entries"][0]["reason"], "neighbor_daily_limit");
    assert_eq!(page["entries"][0]["limit"], 9);
    assert_eq!(page["entries"][0]["resets_at"], 1000);
    let old_generation = service.neighbor_history_generation();
    service.neighbor_identity_changed();
    assert!(matches!(
        service
            .append_neighbor_history(
                &metadata,
                &friendship,
                entry(2, "in", Kind::Question, None),
                old_generation
            )
            .await,
        Err(Error::Revoked)
    ));
    assert!(matches!(
        service
            .update_neighbor_history_status(
                &metadata,
                &outgoing.message_id,
                DeliveryStatus::Delivered,
                None,
                old_generation
            )
            .await,
        Err(Error::Revoked)
    ));
    assert_eq!(
        metadata.history_page(&friendship, None, 1).await.unwrap()["entries"][0]["delivery_status"],
        "not_delivered"
    );
}
