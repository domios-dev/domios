use codeterm_types::notification::{
    adopt_caller_actions, answer_actions, CallerAction, NotificationAction, OfferedChoice,
};

use super::*;
use crate::notification::store::{NotificationProvenance, NotificationSeverity, NotificationStore};

fn choices(labels: &[&str]) -> Vec<NotificationAction> {
    adopt_caller_actions(
        answer_actions(&labels.iter().map(|l| l.to_string()).collect::<Vec<_>>())
            .expect("distinct non-empty labels are a legal choice set"),
    )
}

fn asked_from(tab_id: Option<&str>) -> (NotificationStore, String) {
    let mut store = NotificationStore::new(10);
    let id = store.add_ext(
        "Which branch?".into(),
        None,
        NotificationSeverity::Info,
        NotificationProvenance {
            tab_id: tab_id.map(str::to_string),
            ..Default::default()
        },
        crate::notification::store::Ask::offering(Some(choices(&["Rebase", "Merge"]))),
        None,
        None,
    );
    (store, id)
}

/// The kind has no field for a command name or arguments, so a stored record
/// cannot grow one: naming either is a parse rejection, not a trimmed field.
#[test]
fn notify_answer_kind_cannot_express_a_command_or_arguments() {
    for smuggled in [
        serde_json::json!({"type": "answer", "label": "Yes", "value": "retired_provider_action"}),
        serde_json::json!({"type": "answer", "label": "Yes", "args": {"promptId": "p1"}}),
        serde_json::json!({"type": "answer", "label": "Yes", "value": "x", "args": {}}),
        serde_json::json!({"type": "answer", "label": "Yes", "command": "x"}),
    ] {
        assert!(
            serde_json::from_value::<CallerAction>(smuggled.clone()).is_err(),
            "an answer action must not be able to express {smuggled}"
        );
    }

    let serialized = serde_json::to_value(NotificationAction::Answer {
        label: "Yes".into(),
    })
    .expect("an answer action serializes");
    // Sorted, because serde_json's key order is the map's, not the insertion
    // order — the claim is which fields exist, never the order they serialise in.
    let mut fields: Vec<&str> = serialized
        .as_object()
        .expect("an action is a JSON object")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        vec!["label", "type"],
        "the whole wire shape of a choice is its kind and its label"
    );
}

#[test]
fn detached_system_choice_requires_the_internal_answer_path() {
    let (mut store, id) = asked_from(None);
    assert_eq!(
        store.record_answer(&id, "Merge"),
        AnswerAdmission::Undeliverable
    );
    assert_eq!(
        store.record_detached_answer(&id, "Merge"),
        AnswerAdmission::Recorded("Merge".into())
    );
    assert_eq!(
        store.record_detached_answer(&id, "Rebase"),
        AnswerAdmission::AlreadyAnswered("Merge".into())
    );
}

/// An invented kind is refused where it is parsed, so it can never travel as a
/// string that some later click handler interprets.
#[test]
fn notify_answer_unknown_kind_is_a_parse_rejection() {
    for unknown in ["approve", "run", "Answer", ""] {
        let raw = serde_json::json!({"type": unknown, "label": "Yes"});
        assert!(
            serde_json::from_value::<CallerAction>(raw).is_err(),
            "{unknown} is outside the closed set of action kinds"
        );
    }
    assert!(serde_json::from_value::<CallerAction>(
        serde_json::json!({"type": "answer", "label": "Yes"})
    )
    .is_ok());
}

/// The asking tab is whatever provenance the record was created with — never
/// the environment, and never anything re-read when the answer is tapped.
#[test]
fn notify_answer_records_the_asking_tab_at_send_time() {
    std::env::set_var("CODETERM_TAB_ID", "environment-tab");
    let (store, id) = asked_from(Some("asking-tab"));

    let slot = store
        .answer_slot(&id)
        .expect("a choice opens an answer slot");
    assert_eq!(slot.asked_from_tab.as_deref(), Some("asking-tab"));
    assert_eq!(slot.answered, None);
    assert!(!slot.is_undeliverable());
    std::env::remove_var("CODETERM_TAB_ID");
}

/// A question with no answering tab is born undeliverable rather than offering
/// buttons that would quietly do nothing.
#[test]
fn notify_answer_without_an_asking_tab_is_born_undeliverable() {
    let (mut store, id) = asked_from(None);

    let slot = store
        .answer_slot(&id)
        .expect("a choice opens an answer slot");
    assert!(
        slot.is_undeliverable(),
        "the panel renders this card as undeliverable"
    );
    assert_eq!(
        store.record_answer(&id, "Rebase"),
        AnswerAdmission::Undeliverable
    );
    assert_eq!(
        store.answer_slot(&id).and_then(|s| s.answered.clone()),
        None
    );
}

/// Answering resolves nothing: the tab recorded at send time is the only tab,
/// so once it is gone the card is visibly undeliverable instead of inert.
#[test]
fn notify_answer_undeliverable_once_the_asking_tab_is_gone() {
    let (mut store, id) = asked_from(Some("asking-tab"));
    store.forget_answer_tab("other-tab");
    assert!(!store.answer_slot(&id).unwrap().is_undeliverable());

    store.forget_answer_tab("asking-tab");

    let slot = store.answer_slot(&id).expect("the record outlives its tab");
    assert_eq!(slot.asked_from_tab, None);
    assert!(slot.is_undeliverable());
    assert_eq!(
        store.record_answer(&id, "Rebase"),
        AnswerAdmission::Undeliverable
    );
}

/// One answer per notification, held in the store: a second attempt is refused
/// and the stored answer is exactly what the first attempt recorded.
#[test]
fn notify_answer_takes_exactly_one_answer_per_notification() {
    let (mut store, id) = asked_from(Some("asking-tab"));

    assert_eq!(
        store.record_answer(&id, "Rebase"),
        AnswerAdmission::Recorded("Rebase".into())
    );
    assert_eq!(
        store.record_answer(&id, "Merge"),
        AnswerAdmission::AlreadyAnswered("Rebase".into())
    );
    assert_eq!(
        store.record_answer(&id, "Rebase"),
        AnswerAdmission::AlreadyAnswered("Rebase".into())
    );
    assert_eq!(
        store
            .answer_slot(&id)
            .and_then(|slot| slot.answered.clone()),
        Some("Rebase".into()),
        "the stored answer is unchanged by every later attempt"
    );
    assert_eq!(
        store
            .answer_slot(&id)
            .and_then(|slot| slot.answered_via.as_deref()),
        Some("notification")
    );
}

/// An answer already recorded is history: closing the tab afterwards does not
/// rewrite what the user chose.
#[test]
fn notify_answer_survives_the_asking_tab_closing_afterwards() {
    let (mut store, id) = asked_from(Some("asking-tab"));
    store.record_answer(&id, "Merge");

    store.forget_answer_tab("asking-tab");

    let slot = store.answer_slot(&id).expect("the record outlives its tab");
    assert_eq!(slot.answered.as_deref(), Some("Merge"));
    assert_eq!(slot.asked_from_tab.as_deref(), Some("asking-tab"));
    assert!(!slot.is_undeliverable());
}

/// Only a label that was actually offered can be recorded, so an answer cannot
/// be invented by whoever calls the store.
#[test]
fn notify_answer_refuses_a_label_that_was_never_offered() {
    let (mut store, id) = asked_from(Some("asking-tab"));

    assert_eq!(
        store.record_answer(&id, "Delete"),
        AnswerAdmission::NotOffered
    );
    assert_eq!(
        store.record_answer("no-such-record", "Rebase"),
        AnswerAdmission::NotOffered
    );
    assert_eq!(
        store.answer_slot(&id).and_then(|s| s.answered.clone()),
        None
    );
}

/// A notification that offers no choice has no slot at all, so nothing about it
/// can be answered.
#[test]
fn notify_answer_absent_when_no_choice_is_offered() {
    let mut store = NotificationStore::new(10);
    let id = store.add_ext(
        "Build finished".into(),
        None,
        NotificationSeverity::Info,
        NotificationProvenance {
            tab_id: Some("asking-tab".into()),
            ..Default::default()
        },
        crate::notification::store::Ask::default(),
        None,
        None,
    );
    assert!(store.answer_slot(&id).is_none());
    assert_eq!(
        store.record_answer(&id, "Rebase"),
        AnswerAdmission::NotOffered
    );
}

/// Labels are the whole payload of a choice, so a set no user could tell apart
/// is refused where the set is built.
#[test]
fn notify_answer_choice_set_refuses_blank_and_duplicate_labels() {
    for rejected in [
        vec!["Rebase".to_string(), String::new()],
        vec!["   ".to_string()],
        vec!["Rebase".to_string(), "Rebase".to_string()],
        vec!["Rebase".to_string(), " rebase ".to_string()],
    ] {
        assert!(
            answer_actions(&rejected).is_err(),
            "{rejected:?} is not a choice set a user could answer"
        );
    }
    let accepted = answer_actions(&["Rebase".to_string(), "Merge".to_string()])
        .expect("distinct non-empty labels are legal");
    assert_eq!(
        accepted
            .iter()
            .filter_map(OfferedChoice::answer_label)
            .collect::<Vec<_>>(),
        vec!["Rebase", "Merge"]
    );
}
