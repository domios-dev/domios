use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

fn entry(id: usize, direction: &str, kind: Kind, reply: Option<String>) -> Entry {
    Entry {
        handling: Default::default(),
        received_by: None,
        flags: Vec::new(),
        flag_callers: Vec::new(),
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
        submission: None,
        delivery_status: (direction == "out").then_some(DeliveryStatus::Pending),
    }
}

#[tokio::test]
async fn linked_outcomes_require_accepted_submission_and_reconcile_delivery_failure_reasons() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let incoming = entry(1, "in", Kind::Question, None);
    let outgoing = entry(2, "out", Kind::Answer, Some(incoming.message_id.clone()));
    let generation = service.neighbor_history_generation();
    for row in [incoming, outgoing.clone()] {
        service
            .append_neighbor_history(&metadata, &friendship, row, generation)
            .await
            .unwrap();
    }
    assert!(
        metadata.history_page(&friendship, None, 50).await.unwrap()["entries"][1]["outcome"]
            .is_null()
    );
    for submission in [
        Submission::Unconfirmed,
        Submission::Refused,
        Submission::Accepted,
    ] {
        service
            .update_neighbor_history_status(
                &metadata,
                &outgoing.message_id,
                HistoryReceipt {
                    delivery: DeliveryStatus::Pending.into(),
                    rejection: None,
                    submission: Some(submission),
                },
                generation,
            )
            .await
            .unwrap();
        let page = metadata.history_page(&friendship, None, 50).await.unwrap();
        assert_eq!(
            page["entries"][1]["outcome"],
            if submission == Submission::Accepted {
                json!("answered")
            } else {
                Value::Null
            }
        );
    }
    service
        .update_neighbor_history_status(
            &metadata,
            &outgoing.message_id,
            DeliveryResult {
                status: DeliveryStatus::NotDelivered,
                reason: Some(codeterm_neighbors::wire::DeliveryReason::KeyRevoked),
            }
            .into(),
            generation,
        )
        .await
        .unwrap();
    let page = metadata.history_page(&friendship, None, 50).await.unwrap();
    assert_eq!(page["entries"][0]["reason"], "key_revoked");
    assert!(page["entries"][1]["outcome"].is_null());
    let refusal = entry(3, "out", Kind::Refusal, Some(format!("{:032x}", 1)));
    service
        .append_neighbor_history(&metadata, &friendship, refusal.clone(), generation)
        .await
        .unwrap();
    service
        .update_neighbor_history_status(
            &metadata,
            &refusal.message_id,
            HistoryReceipt {
                delivery: DeliveryStatus::NotDelivered.into(),
                rejection: super::super::delivery::Rejection::from_error(Error::DailyLimit {
                    limit: 1,
                    resets_at: 100,
                }),
                submission: Some(Submission::Refused),
            },
            generation,
        )
        .await
        .unwrap();
    assert!(
        metadata.history_page(&friendship, None, 50).await.unwrap()["entries"][2]["outcome"]
            .is_null()
    );
    service
        .update_neighbor_history_status(
            &metadata,
            &refusal.message_id,
            HistoryReceipt {
                delivery: DeliveryStatus::Pending.into(),
                rejection: None,
                submission: Some(Submission::Accepted),
            },
            generation,
        )
        .await
        .unwrap();
    assert_eq!(
        metadata.history_page(&friendship, None, 50).await.unwrap()["entries"][2]["outcome"],
        "refused"
    );
    let page = metadata.history_page(&friendship, None, 50).await.unwrap();
    assert_eq!(page["entries"][0]["delivery_status"], "pending");
    assert!(page["entries"][0]["reason"].is_null());
    assert!(page["entries"][0]["limit"].is_null());
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
    let mut refusal = entry(300, "out", Kind::Refusal, Some(format!("{:032x}", 299)));
    refusal.submission = Some(Submission::Accepted);
    history.append(refusal).unwrap();
    let incoming = history
        .entries
        .iter()
        .find(|entry| entry.at == 299)
        .unwrap();
    assert_eq!(incoming.outcome.as_deref(), Some("refused"));
    assert_eq!(incoming.reason.as_deref(), Some("opaque-300"));
    let mut answer = entry(301, "out", Kind::Answer, Some(format!("{:032x}", 298)));
    answer.submission = Some(Submission::Accepted);
    history.append(answer).unwrap();
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
        large.body = "\u{0001}".repeat(codeterm_neighbors::wire::MAX_PLAINTEXT_BYTES);
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
            DeliveryStatus::Delivered.into(),
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
        Err(Error::FriendshipEnded)
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

#[tokio::test]
async fn accepted_friendship_reopens_retained_history_but_stale_active_epochs_do_not() {
    assert!(Error::FriendshipEnded
        .to_string()
        .starts_with("friendship_ended:"));
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let generation = service.neighbor_history_generation();
    service
        .append_neighbor_history(
            &metadata,
            &friendship,
            entry(1, "out", Kind::Question, None),
            generation,
        )
        .await
        .unwrap();
    service
        .end_neighbor_history(&metadata, &friendship, generation)
        .await
        .unwrap();
    assert_eq!(
        service
            .append_neighbor_history(
                &metadata,
                &friendship,
                entry(2, "out", Kind::Question, None),
                generation
            )
            .await,
        Err(Error::FriendshipEnded)
    );
    let active = |since| json!({"friends":[{"friendship_id":friendship,"since":since}]});
    service
        .refresh_active_neighbor_history(&metadata, &active(200), generation)
        .await
        .unwrap();
    service
        .append_neighbor_history(
            &metadata,
            &friendship,
            entry(2, "out", Kind::Question, None),
            generation,
        )
        .await
        .unwrap();
    let page = metadata.history_page(&friendship, None, 50).await.unwrap();
    assert_eq!(page["ended"], false);
    assert_eq!(page["entries"].as_array().unwrap().len(), 2);
    assert_eq!(page["entries"][1]["body"], "opaque-1");
    service
        .end_neighbor_history(&metadata, &friendship, generation)
        .await
        .unwrap();
    for since in [100, 200] {
        service
            .refresh_active_neighbor_history(&metadata, &active(since), generation)
            .await
            .unwrap();
        assert_eq!(
            metadata.history_page(&friendship, None, 50).await.unwrap()["ended"],
            true
        );
    }
    service
        .refresh_active_neighbor_history(&metadata, &active(300), generation)
        .await
        .unwrap();
    assert_eq!(
        metadata.history_page(&friendship, None, 50).await.unwrap()["ended"],
        false
    );
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
            HistoryReceipt {
                delivery: DeliveryStatus::NotDelivered.into(),
                rejection: super::super::delivery::Rejection::from_error(Error::DailyLimit {
                    limit: 9,
                    resets_at: 1000,
                }),
                submission: Some(Submission::Refused),
            },
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    service
        .update_neighbor_history_status(
            &metadata,
            &outgoing.message_id,
            DeliveryStatus::NotDelivered.into(),
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
                DeliveryStatus::Delivered.into(),
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
