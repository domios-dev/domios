use std::cell::RefCell;
use std::sync::Arc;

use codeterm_types::message_markers;
use codeterm_types::notification::{adopt_caller_actions, answer_actions, NotificationAction};
use parking_lot::Mutex;

use super::*;
use crate::notification::push::PushBounds;
use crate::notification::store::{NotificationProvenance, NotificationSeverity, NotificationStore};
use crate::service::messenger::Messenger;

const BOUNDS: PushBounds = PushBounds::for_ceiling(4096);
const QUESTION: &str = "Which branch?";
const CHOSEN: &str = "Rebase";
const ASKING_TAB: &str = "tab-asking-7";

fn choices(labels: &[&str]) -> Vec<NotificationAction> {
    adopt_caller_actions(
        answer_actions(&labels.iter().map(|l| l.to_string()).collect::<Vec<_>>())
            .expect("distinct non-empty labels are a legal choice set"),
    )
}

/// A store holding one question, asked from `tab_id` (or from a tab that is
/// already gone when `tab_id` is `None`).
fn asked_from(tab_id: Option<&str>) -> (NotifState, String) {
    let mut store = NotificationStore::new(10);
    let id = store.add_ext(
        QUESTION.into(),
        None,
        NotificationSeverity::Info,
        NotificationProvenance {
            tab_id: tab_id.map(str::to_string),
            ..Default::default()
        },
        crate::notification::store::Ask::offering(Some(choices(&[CHOSEN, "Merge"]))),
        None,
        None,
    );
    (Arc::new(Mutex::new(store)), id)
}

/// What the messenger boundary was handed. The spy stands exactly where
/// `Messenger::deliver` stands, so a delivery that bypassed it records nothing.
#[derive(Default)]
struct Spy {
    calls: RefCell<Vec<(String, String)>>,
}

impl Spy {
    fn accept(&self, to_tab: &str, envelope: MessageEnvelope, body: &str) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push((to_tab.to_string(), Messenger::from(envelope).wrap(body)));
        Ok(())
    }

    fn calls(&self) -> Vec<(String, String)> {
        self.calls.borrow().clone()
    }
}

/// The one host-event frame that already exists, derived rather than restated:
/// an answer must ride the same marker family, not a convention of its own.
fn host_event_kind() -> String {
    let wrapped = Messenger::from(MessageEnvelope::from_codeterm()).wrap("probe");
    message_markers::parse(&wrapped)
        .expect("the host envelope composes a marker")
        .kind
}

#[test]
fn answer_delivery_carries_the_recorded_tab_and_the_chosen_label() {
    let (store, id) = asked_from(Some(ASKING_TAB));
    store.lock().record_answer(&id, CHOSEN);

    let spy = Spy::default();
    let outcome = deliver(&store, &id, |to_tab, envelope, body| {
        spy.accept(to_tab, envelope, body)
    });

    assert_eq!(outcome, AnswerDeliveryOutcome::Delivered);
    let calls = spy.calls();
    assert_eq!(calls.len(), 1, "one recorded answer is one delivery");
    let (to_tab, wrapped) = &calls[0];
    assert_eq!(
        to_tab, ASKING_TAB,
        "the answer goes to the tab captured when the question was sent"
    );

    let marker =
        message_markers::parse(wrapped).expect("the envelope composed the marker, not this module");
    assert_eq!(
        marker.kind,
        host_event_kind(),
        "an answer rides the host-event frame that already exists"
    );
    assert_eq!(marker.attr("context"), Some(ANSWER_CONTEXT));
    assert!(
        marker.payload.contains(CHOSEN),
        "the chosen label the test supplied must round-trip into the body"
    );
    assert!(
        marker.payload.contains(QUESTION),
        "the body must name the question it answers"
    );
    assert!(
        marker.payload.contains(&id),
        "the body must name the record it answers"
    );
}

#[test]
fn answer_delivery_goes_through_the_existing_messenger_boundary() {
    let (store, id) = asked_from(Some(ASKING_TAB));
    store.lock().record_answer(&id, CHOSEN);

    let plan = store.lock().answer_delivery_plan(&id);
    let AnswerDeliveryPlan::Owed(plan) = plan else {
        panic!("a recorded answer for a live tab is owed a delivery");
    };
    // The envelope is one of the pre-existing kinds, so the existing kind ->
    // submit-entry mapping covers it and no new submit variant is introduced.
    assert_eq!(
        plan.envelope().kind,
        MessageEnvelope::from_codeterm().kind,
        "the answer reuses an existing message kind"
    );

    let spy = Spy::default();
    let outcome = deliver(&store, &id, |to_tab, envelope, body| {
        spy.accept(to_tab, envelope, body)
    });
    assert_eq!(outcome, AnswerDeliveryOutcome::Delivered);
    assert_eq!(
        spy.calls().len(),
        1,
        "delivery reaches the messenger boundary rather than a private path"
    );
}

#[test]
fn answer_delivery_never_attempted_for_a_dead_recorded_tab() {
    let (store, id) = asked_from(Some(ASKING_TAB));
    store.lock().forget_answer_tab(ASKING_TAB);

    let admission = store.lock().record_answer(&id, CHOSEN);
    assert_eq!(
        admission,
        crate::notification::answer::AnswerAdmission::Undeliverable,
        "a question whose tab is gone takes no answer, and swallows nothing"
    );

    let spy = Spy::default();
    let outcome = deliver(&store, &id, |to_tab, envelope, body| {
        spy.accept(to_tab, envelope, body)
    });
    assert_eq!(outcome, AnswerDeliveryOutcome::NotAttempted);
    assert!(spy.calls().is_empty(), "a dead tab is never sent to");

    let slot = store
        .lock()
        .answer_slot(&id)
        .cloned()
        .expect("the slot stands");
    assert!(
        slot.is_undeliverable(),
        "the card says so instead of claiming an answer"
    );
    assert!(!slot.delivered);
}

#[test]
fn answer_delivery_is_idempotent_after_a_successful_send() {
    let (store, id) = asked_from(Some(ASKING_TAB));
    store.lock().record_answer(&id, CHOSEN);

    let spy = Spy::default();
    let first = deliver(&store, &id, |to_tab, envelope, body| {
        spy.accept(to_tab, envelope, body)
    });
    let second = deliver(&store, &id, |to_tab, envelope, body| {
        spy.accept(to_tab, envelope, body)
    });

    assert_eq!(first, AnswerDeliveryOutcome::Delivered);
    assert_eq!(second, AnswerDeliveryOutcome::AlreadyDelivered);
    assert_eq!(
        spy.calls().len(),
        1,
        "a retry after a successful delivery duplicates nothing"
    );
    assert!(second.reached_tab());
}

#[test]
fn answer_delivery_failure_surfaces_as_not_delivered() {
    let (store, id) = asked_from(Some(ASKING_TAB));
    store.lock().record_answer(&id, CHOSEN);

    let refusal = "agent_unavailable: the tab has no PTY";
    let outcome = deliver(&store, &id, |_, _, _| Err(refusal.to_string()));

    assert_eq!(outcome, AnswerDeliveryOutcome::Failed(refusal.to_string()));
    assert!(!outcome.reached_tab(), "a refused send is never delivered");
    assert_eq!(
        outcome.failure(),
        Some(refusal),
        "the reason is not swallowed"
    );

    let slot = store
        .lock()
        .answer_slot(&id)
        .cloned()
        .expect("the slot stands");
    assert_eq!(slot.answered.as_deref(), Some(CHOSEN));
    assert!(
        slot.is_undelivered(),
        "the record must not read as answered-and-delivered"
    );

    // The answer stands, so a later attempt may still carry it — once.
    let spy = Spy::default();
    let retry = deliver(&store, &id, |to_tab, envelope, body| {
        spy.accept(to_tab, envelope, body)
    });
    assert_eq!(retry, AnswerDeliveryOutcome::Delivered);
    assert_eq!(spy.calls().len(), 1);
    assert!(
        store
            .lock()
            .answer_slot(&id)
            .expect("the slot stands")
            .delivered
    );
}

/// A question that was actually pushed to a channel, and the placement that
/// channel reported for the message carrying its buttons.
fn pushed_to_channel(locator: &str) -> (NotifState, String) {
    let mut store = NotificationStore::new(10);
    let id = store.add_ext(
        QUESTION.into(),
        None,
        NotificationSeverity::Info,
        NotificationProvenance {
            tab_id: Some(ASKING_TAB.to_string()),
            ..Default::default()
        },
        crate::notification::store::Ask::offering(Some(choices(&[CHOSEN, "Merge"]))),
        None,
        Some(crate::notification::push::NotifyLevel::ActionNeeded),
    );
    store.remember_channel_placement(&id, "a-channel-with-a-message-to-rewrite", locator);
    (Arc::new(Mutex::new(store)), id)
}

/// A tap in the panel records the answer on the record, not on the surface that
/// showed it, so the channel that also showed it stops showing it as open.
#[test]
fn spent_question_answered_in_the_panel_still_spends_the_channel() {
    let (store, id) = pushed_to_channel("chat-9:message-4");
    // Nothing here goes near a channel: this is the in-app answer path.
    store.lock().record_answer(&id, CHOSEN);

    let spy = Spy::default();
    let spent: RefCell<Vec<(String, String, String)>> = RefCell::new(Vec::new());
    let outcome = settle(
        &store,
        &id,
        |to_tab, envelope, body| spy.accept(to_tab, envelope, body),
        |spent_question| {
            for placement in &spent_question.placements {
                spent.borrow_mut().push((
                    placement.service.clone(),
                    placement.locator.clone(),
                    spent_question.message_for(BOUNDS).text,
                ));
            }
        },
    );

    assert_eq!(outcome, AnswerDeliveryOutcome::Delivered);
    let rewrites = spent.borrow().clone();
    assert_eq!(
        rewrites.len(),
        1,
        "the one placed message is rewritten once"
    );
    assert_eq!(rewrites[0].1, "chat-9:message-4");
    assert!(
        rewrites[0].2.contains(CHOSEN),
        "the chosen label the test supplied reaches the rewritten message"
    );
    assert!(
        !store
            .lock()
            .spent_question(&id)
            .expect("a spent question still describes itself")
            .message_for(BOUNDS)
            .offers_choices(),
        "a spent question carries no options to draw"
    );
}

/// The tap is the user's decision; the rewrite is presentation. A channel that
/// refuses the rewrite cannot take the decision back.
#[test]
fn spent_question_a_refused_rewrite_still_records_and_delivers_the_answer() {
    let (store, id) = pushed_to_channel("chat-9:message-4");
    store.lock().record_answer(&id, CHOSEN);

    let spy = Spy::default();
    let refusals: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let outcome = settle(
        &store,
        &id,
        |to_tab, envelope, body| spy.accept(to_tab, envelope, body),
        |spent_question| {
            for placement in &spent_question.placements {
                // Exactly what a channel with no rewrite of its own answers.
                refusals
                    .borrow_mut()
                    .push(format!("{} cannot rewrite", placement.service));
            }
        },
    );

    assert_eq!(
        outcome,
        AnswerDeliveryOutcome::Delivered,
        "a refused rewrite leaves the delivery outcome untouched"
    );
    assert_eq!(refusals.borrow().len(), 1, "the rewrite was attempted");
    assert_eq!(
        store
            .lock()
            .answer_slot(&id)
            .and_then(|s| s.answered.clone()),
        Some(CHOSEN.to_string()),
        "the answer is still recorded"
    );
    assert!(
        store.lock().answer_slot(&id).is_some_and(|s| s.delivered),
        "the answer still reached the tab that asked"
    );
    assert_eq!(spy.calls().len(), 1);
}

/// A question nobody carried to a channel has nothing to spend there.
#[test]
fn spent_question_a_record_no_channel_carried_rewrites_nothing() {
    let (store, id) = asked_from(Some(ASKING_TAB));
    store.lock().record_answer(&id, CHOSEN);

    let spy = Spy::default();
    let attempts = RefCell::new(0);
    let outcome = settle(
        &store,
        &id,
        |to_tab, envelope, body| spy.accept(to_tab, envelope, body),
        |_| *attempts.borrow_mut() += 1,
    );

    assert_eq!(outcome, AnswerDeliveryOutcome::Delivered);
    assert_eq!(
        *attempts.borrow(),
        0,
        "nothing was placed, so nothing is rewritten"
    );
}
