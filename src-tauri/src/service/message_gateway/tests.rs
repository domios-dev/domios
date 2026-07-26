use std::cell::RefCell;

use super::*;

#[derive(Default)]
struct FakeSeam {
    enveloped: RefCell<Vec<(String, String, String)>>,
    texts: RefCell<Vec<(String, String, bool)>>,
    batches: RefCell<Vec<(String, usize)>>,
    keys: RefCell<Vec<(String, String, Vec<KeyStroke>)>>,
    error: Option<String>,
}

impl FakeSeam {
    fn failing(err: &str) -> Self {
        Self {
            error: Some(err.to_string()),
            ..Default::default()
        }
    }

    fn guard(&self) -> Result<(), String> {
        match &self.error {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}

impl DeliverySeam for FakeSeam {
    fn deliver_enveloped(
        &self,
        to_pane: &str,
        envelope: MessageEnvelope,
        body: &str,
    ) -> Result<Delivery, String> {
        self.enveloped.borrow_mut().push((
            to_pane.to_string(),
            Messenger::pty_prefix(&envelope),
            body.to_string(),
        ));
        self.guard()?;
        Ok(Delivery::Pty {
            text: format!("{}{}", Messenger::pty_prefix(&envelope), body),
        })
    }

    fn deliver_text(&self, pane: &str, text: &str, enter: bool) -> Result<(), String> {
        self.texts
            .borrow_mut()
            .push((pane.to_string(), text.to_string(), enter));
        self.guard()
    }

    fn deliver_batch(&self, pane: &str, batch: Vec<SendBatchItem>) -> Result<(), String> {
        self.batches
            .borrow_mut()
            .push((pane.to_string(), batch.len()));
        self.guard()
    }

    fn deliver_keys(&self, pane: &str, text: &str, strokes: &[KeyStroke]) -> Result<(), String> {
        self.keys
            .borrow_mut()
            .push((pane.to_string(), text.to_string(), strokes.to_vec()));
        self.guard()
    }
}

fn msg_with_class(target: &str, class: DeliveryClass) -> OutboundMessage {
    OutboundMessage {
        target: target.to_string(),
        body: MessageBody::Text {
            body: "body".to_string(),
            enter: true,
        },
        envelope: None,
        class,
        origin: MessageOrigin::System,
        idempotency_key: None,
    }
}

#[test]
fn every_delivery_class_dispatches_optimistically() {
    let gateway = MessageGateway::new();
    for class in [
        DeliveryClass::AgentTraffic,
        DeliveryClass::SystemNotice,
        DeliveryClass::UserTurn,
        DeliveryClass::RawKeys,
    ] {
        let fake = FakeSeam::default();
        gateway.submit(&fake, msg_with_class("p1", class)).unwrap();
        assert_eq!(fake.texts.borrow().len(), 1, "{class:?} must not wait");
    }
}

#[test]
fn worker_report_requirement_is_consumed_exactly_once() {
    let gateway = MessageGateway::new();

    gateway.require_worker_report("worker-tab");

    assert!(gateway.take_worker_report_requirement("worker-tab"));
    assert!(!gateway.take_worker_report_requirement("worker-tab"));
}

#[test]
fn explicit_worker_report_clears_the_requirement() {
    let gateway = MessageGateway::new();
    gateway.require_worker_report("worker-tab");

    gateway.clear_worker_report_requirement("worker-tab");

    assert!(!gateway.take_worker_report_requirement("worker-tab"));
}

fn text_msg(target: &str, body: &str, key: Option<&str>) -> OutboundMessage {
    OutboundMessage {
        target: target.to_string(),
        body: MessageBody::Text {
            body: body.to_string(),
            enter: true,
        },
        envelope: None,
        class: DeliveryClass::UserTurn,
        origin: MessageOrigin::ServiceToken,
        idempotency_key: key.map(|k| k.to_string()),
    }
}

#[test]
fn enveloped_submit_forwards_envelope_and_body_byte_identical() {
    let gateway = MessageGateway::new();
    let fake = FakeSeam::default();
    let envelope = MessageEnvelope::pane("Edison", "e8a0ac20");
    let expected_prefix = Messenger::pty_prefix(&envelope);

    let msg = OutboundMessage {
        target: "p1".to_string(),
        body: MessageBody::Text {
            body: "hello".to_string(),
            enter: true,
        },
        envelope: Some(envelope.clone()),
        class: DeliveryClass::AgentTraffic,
        origin: MessageOrigin::LocalPane,
        idempotency_key: None,
    };

    let receipt = gateway.submit(&fake, msg).unwrap();

    let calls = fake.enveloped.borrow();
    assert_eq!(calls.len(), 1, "exactly one enveloped delivery");
    assert_eq!(calls[0].0, "p1", "target forwarded unchanged");
    assert_eq!(calls[0].1, expected_prefix, "envelope forwarded unchanged");
    assert_eq!(calls[0].2, "hello", "body not mutated");
    assert!(fake.texts.borrow().is_empty(), "not routed to plain text");
    assert!(!receipt.dedup_hit);
    assert!(matches!(receipt.delivery, Some(Delivery::Pty { .. })));
}

#[test]
fn plain_text_submit_matches_direct_send_text() {
    let gateway = MessageGateway::new();
    let fake = FakeSeam::default();

    gateway
        .submit(&fake, text_msg("p1", "raw body", None))
        .unwrap();

    let calls = fake.texts.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0], ("p1".to_string(), "raw body".to_string(), true));
    assert!(fake.enveloped.borrow().is_empty(), "not routed to envelope");
}

#[test]
fn batch_submit_forwards_to_batch_path() {
    let gateway = MessageGateway::new();
    let fake = FakeSeam::default();
    let msg = OutboundMessage {
        target: "p2".to_string(),
        body: MessageBody::Batch(vec![
            SendBatchItem {
                text: "one".to_string(),
                enter: false,
                delay_ms: None,
            },
            SendBatchItem {
                text: "two".to_string(),
                enter: true,
                delay_ms: Some(5),
            },
        ]),
        envelope: None,
        class: DeliveryClass::AgentTraffic,
        origin: MessageOrigin::Ui,
        idempotency_key: None,
    };

    gateway.submit(&fake, msg).unwrap();

    let calls = fake.batches.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0], ("p2".to_string(), 2));
}

#[test]
fn keys_submit_forwards_strokes() {
    let gateway = MessageGateway::new();
    let fake = FakeSeam::default();
    let msg = OutboundMessage {
        target: "p3".to_string(),
        body: MessageBody::Keys {
            text: "cmd".to_string(),
            strokes: vec![KeyStroke::Key("enter".to_string()), KeyStroke::Delay(10)],
        },
        envelope: None,
        class: DeliveryClass::RawKeys,
        origin: MessageOrigin::LocalPane,
        idempotency_key: None,
    };

    gateway.submit(&fake, msg).unwrap();

    let calls = fake.keys.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "cmd");
    assert_eq!(calls[0].2.len(), 2);
}

#[test]
fn duplicate_key_returns_cached_receipt_without_second_delivery() {
    let gateway = MessageGateway::new();
    let fake = FakeSeam::default();

    let first = gateway
        .submit(&fake, text_msg("p1", "hi", Some("k1")))
        .unwrap();
    assert!(!first.dedup_hit);

    let second = gateway
        .submit(&fake, text_msg("p1", "hi", Some("k1")))
        .unwrap();
    assert!(second.dedup_hit, "replay flagged as dedup hit");

    assert_eq!(
        fake.texts.borrow().len(),
        1,
        "duplicate submit performed NO second delivery"
    );
}

#[test]
fn distinct_origin_or_key_is_not_deduped() {
    let gateway = MessageGateway::new();
    let fake = FakeSeam::default();

    gateway
        .submit(&fake, text_msg("p1", "hi", Some("k1")))
        .unwrap();
    gateway
        .submit(&fake, text_msg("p1", "hi", Some("k2")))
        .unwrap();

    assert_eq!(fake.texts.borrow().len(), 2, "different keys both deliver");
}

#[test]
fn cached_error_replays_without_second_delivery() {
    let gateway = MessageGateway::new();
    let fake = FakeSeam::failing("Pane p9 not found or has no PTY");

    let first = gateway
        .submit(&fake, text_msg("p9", "hi", Some("k1")))
        .unwrap_err();
    let second = gateway
        .submit(&fake, text_msg("p9", "hi", Some("k1")))
        .unwrap_err();

    assert_eq!(first, second, "same typed error replayed");
    assert_eq!(fake.texts.borrow().len(), 1, "no second delivery attempt");
}

#[test]
fn pane_not_found_and_delivered_unverified_are_distinct() {
    let gateway = MessageGateway::new();

    let not_found = gateway
        .submit(
            &FakeSeam::failing("Pane p9 not found or has no PTY"),
            text_msg("p9", "hi", None),
        )
        .unwrap_err();
    assert!(matches!(not_found, DeliveryError::PaneNotFound { .. }));

    let unverified = gateway
        .submit(
            &FakeSeam::failing("submit not confirmed after 5s; screen_tail=\"> hi\""),
            text_msg("p1", "hi", None),
        )
        .unwrap_err();
    assert!(matches!(
        unverified,
        DeliveryError::DeliveredUnverified { .. }
    ));

    assert_ne!(
        std::mem::discriminant(&not_found),
        std::mem::discriminant(&unverified),
        "the two error variants are distinct"
    );
}

#[test]
fn classify_maps_structural_markers() {
    assert!(matches!(
        classify("p", "Pane p not found or has no PTY".to_string()),
        DeliveryError::PaneNotFound { .. }
    ));
    assert!(matches!(
        classify("p", "Pane p closed before PTY link".to_string()),
        DeliveryError::PaneNotFound { .. }
    ));
    assert!(matches!(
        classify("p", "submit not confirmed after 5s".to_string()),
        DeliveryError::DeliveredUnverified { .. }
    ));
    assert!(matches!(
        classify("p", "some other failure".to_string()),
        DeliveryError::Transport { .. }
    ));
}

#[test]
fn delivery_class_semantics_are_documented_and_exhaustive() {
    // Compile-time proof every class variant is handled; a new variant that has
    // no documented semantic here fails to compile until it is added.
    fn describe(class: DeliveryClass) -> &'static str {
        match class {
            DeliveryClass::UserTurn => "human-authored turn",
            DeliveryClass::AgentTraffic => "agent-to-agent traffic",
            DeliveryClass::SystemNotice => "codeterm-originated notice",
            DeliveryClass::RawKeys => "uninterpreted key bytes",
        }
    }

    for class in [
        DeliveryClass::UserTurn,
        DeliveryClass::AgentTraffic,
        DeliveryClass::SystemNotice,
        DeliveryClass::RawKeys,
    ] {
        assert!(!describe(class).is_empty());
    }
}

#[test]
fn idempotency_store_evicts_beyond_capacity() {
    let mut store = IdempotencyStore::new(2);
    let ok = |t: &str| {
        Ok(DeliveryReceipt {
            delivery: None,
            dedup_hit: false,
            idempotency_key: Some(t.to_string()),
        })
    };
    store.put("a".to_string(), ok("a"));
    store.put("b".to_string(), ok("b"));
    store.put("c".to_string(), ok("c"));

    assert!(store.get("a").is_none(), "oldest entry evicted");
    assert!(store.get("b").is_some());
    assert!(store.get("c").is_some());
}
