use super::*;
use crate::authority;
use crate::push;
use crate::store::{
    Ask, NotificationProvenance, NotificationSeverity, NotificationStore, PushRequest,
};
use codeterm_types::config::schema::AppConfig;
use codeterm_types::notification::{adopt_caller_actions, answer_actions, CallerAction};

const ASKING_TAB: &str = "tab_the_one_that_asked";
const CHOSEN: &str = "Rebase";
const DECLINED: &str = "Merge";

/// The level every question here is asked at. The record carries it and the
/// tap opens it, from this one constant, so the two cannot drift apart in the
/// fixture and leave the proportionality assertion testing nothing.
const ASKED_AT: NotifyLevel = NotifyLevel::ActionNeeded;

/// Whether one cell of the levels-by-destinations grid is open.
fn routed(cfg: &AppConfig, level: NotifyLevel, column: NotifyDestination<'_>) -> bool {
    cfg.notifications
        .agent_levels
        .iter()
        .find(|row| NotifyLevel::parse(&row.level) == Some(level))
        .is_some_and(|row| row.allows(column))
}

/// Put one cell back the way a fresh install represents it, so a test can undo
/// exactly what an offer opened and compare the WHOLE config against baseline.
/// A closed channel column is an absent key, not a stored `false`, so restoring
/// it means removing the key the tap inserted.
fn shut_cell(cfg: &mut AppConfig, level: NotifyLevel, column: NotifyDestination<'_>) {
    let Some(row) = cfg
        .notifications
        .agent_levels
        .iter_mut()
        .find(|row| NotifyLevel::parse(&row.level) == Some(level))
    else {
        return;
    };
    match column {
        NotifyDestination::Channel(service) => {
            row.channels.remove(service);
        }
        local => row.set(local, false),
    }
}

fn choice_set(labels: &[&str]) -> Vec<CallerAction> {
    answer_actions(&labels.iter().map(|l| l.to_string()).collect::<Vec<_>>())
        .expect("distinct non-empty labels are a legal choice set")
}

fn ask(labels: &[&str], offer: Option<SurfaceOffer>) -> Ask {
    Ask {
        actions: Some(adopt_caller_actions(choice_set(labels))),
        offer,
        condition: None,
    }
}

/// One levelled question, stored exactly as the daemon stores it: the asking
/// tab is captured from resolved provenance at send time.
fn asked_question(store: &mut NotificationStore, ask: Ask) -> String {
    let provenance = NotificationProvenance {
        tab_id: Some(ASKING_TAB.to_string()),
        tab_title: Some("Worker lane".to_string()),
        ..Default::default()
    };
    let admitted = store.add_push(PushRequest {
        scope: push::scope_key(&provenance),
        level: ASKED_AT,
        event_key: push::event_key(None, "Which branch?"),
        title: "Which branch?".to_string(),
        body: None,
        severity: NotificationSeverity::Info,
        provenance,
        per_scope_limit: 8,
        ask,
    });
    admitted.id().to_string()
}

/// THE AUTHORITY RULE. Nothing an agent reaches on its own may change where a
/// notification goes, and the one thing that may is unreachable without a tap.
#[test]
fn channel_ask_authority_refuses_every_unattended_surface_change() {
    for key in [
        authority::SURFACE_ROOT,
        "notifications.desktop",
        "notifications.show_bell",
        "notifications.channels.a-service.recipients",
        "notifications.agent_levels",
    ] {
        assert!(
            admit_unattended_change(key).is_err(),
            "{key} decides where notifications go and must not move unattended"
        );
        assert!(authority::is_surface_path(key));
    }
    for key in [
        "terminal.font_size",
        "mesh.enabled",
        "integrations",
        "api.port",
    ] {
        assert!(admit_unattended_change(key).is_ok());
        assert!(!authority::is_surface_path(key));
    }

    // The proof that unlocks a change exists only for a first-and-only tap.
    assert!(tap_proof(&AnswerAdmission::Recorded(CHOSEN.into())).is_some());
    for without in [
        AnswerAdmission::AlreadyAnswered(CHOSEN.into()),
        AnswerAdmission::Undeliverable,
        AnswerAdmission::NotOffered,
    ] {
        assert!(
            tap_proof(&without).is_none(),
            "{without:?} is not a tap and may not change a surface"
        );
    }

    // A tap may change only the offered cell for the shown level, plus the desktop master switch that gates it.
    let before = AppConfig::default();
    let mut after = before.clone();
    let proof = tap_proof(&AnswerAdmission::Recorded(CHOSEN.into())).expect("a tap");
    apply(
        proof,
        &mut after.notifications,
        &SurfaceTarget::Desktop,
        ASKED_AT,
    );

    assert!(
        routed(&after, ASKED_AT, NotifyDestination::Desktop),
        "the level the question demonstrated is the level the tap opened"
    );
    assert!(
        after.notifications.desktop,
        "the master switch the cell is ANDed with opens too, or the grant is a silent no-op"
    );
    // The proportionality criterion, and the reason it is only one level: a tap on one question must not route a kind of notification the user was never shown.
    for unshown in NotifyLevel::ALL.into_iter().filter(|it| *it != ASKED_AT) {
        assert!(
            !routed(&after, unshown, NotifyDestination::Desktop),
            "{} was never demonstrated to the user and stays shut",
            unshown.as_str()
        );
    }

    // Put back exactly the cell and the master, and the whole config must be
    // identical — which is what makes the blast radius stated rather than hoped.
    shut_cell(&mut after, ASKED_AT, NotifyDestination::Desktop);
    after.notifications.desktop = before.notifications.desktop;
    assert_eq!(
        serde_json::to_value(&after).unwrap(),
        serde_json::to_value(&before).unwrap(),
        "an accepted offer opens one cell and moves nothing else anywhere"
    );
}

/// Offer and apply: a tapped answer applies the change, and an untapped or
/// declined offer changes nothing at all.
#[test]
fn channel_ask_offer_applies_on_the_tap_and_is_inert_otherwise() {
    let service = "opaque-channel".to_string();
    let offer = adopt_offer(
        SurfaceTarget::Channel {
            service: service.clone(),
        },
        CHOSEN,
        &choice_set(&[CHOSEN, DECLINED]),
    )
    .expect("the accepting label is one of the offered choices");

    // An offer may only accept on a label this notification actually offers.
    assert!(adopt_offer(
        SurfaceTarget::Desktop,
        "Some other words",
        &choice_set(&[CHOSEN, DECLINED])
    )
    .is_err());

    let baseline = AppConfig::default();
    let column = NotifyDestination::Channel(&service);
    assert!(
        !routed(&baseline, ASKED_AT, column),
        "a fresh install routes this level nowhere but the bell"
    );

    let mut store = NotificationStore::new(10);
    let id = asked_question(&mut store, ask(&[CHOSEN, DECLINED], Some(offer.clone())));
    assert_eq!(
        store.surface_offer(&id),
        Some(offer.clone()),
        "the record carries the offer a tap would apply"
    );

    // Declined: the recording is real, but it is not the accepting label.
    let mut declined = baseline.clone();
    let admission = store.record_answer(&id, DECLINED);
    assert_eq!(admission, AnswerAdmission::Recorded(DECLINED.to_string()));
    let proof = tap_proof(&admission).expect("a decline is still a first tap");
    if offer.accepted_by(DECLINED) {
        apply(proof, &mut declined.notifications, &offer.target, ASKED_AT);
    }
    assert!(
        !routed(&declined, ASKED_AT, column),
        "a declined offer changes nothing"
    );
    assert_eq!(
        serde_json::to_value(&declined).unwrap(),
        serde_json::to_value(&baseline).unwrap()
    );

    // Accepted: the same seam, on the label the offer named.
    let mut accepted = baseline.clone();
    let mut fresh = NotificationStore::new(10);
    let fresh_id = asked_question(&mut fresh, ask(&[CHOSEN, DECLINED], Some(offer.clone())));
    let admission = fresh.record_answer(&fresh_id, CHOSEN);
    let proof = tap_proof(&admission).expect("the accepting tap");
    assert!(offer.accepted_by(CHOSEN));
    apply(proof, &mut accepted.notifications, &offer.target, ASKED_AT);
    assert!(
        routed(&accepted, ASKED_AT, column),
        "the accepted offer routed this level to the channel"
    );
    // One tap, one level. Routing routine progress somewhere the user was never
    // shown is the escalation this whole rule exists to refuse.
    for unshown in NotifyLevel::ALL.into_iter().filter(|it| *it != ASKED_AT) {
        assert!(
            !routed(&accepted, unshown, column),
            "{} was never demonstrated to the user and stays shut",
            unshown.as_str()
        );
    }
    // Whether a level reaches a channel is the grid's answer alone. The
    // channel's own entry says WHO, so a tap must not mint one.
    assert!(
        !accepted.notifications.channels.contains_key(&service),
        "opening a column is not an audience, and a tap writes no channel entry"
    );

    shut_cell(&mut accepted, ASKED_AT, column);
    assert_eq!(
        serde_json::to_value(&accepted).unwrap(),
        serde_json::to_value(&baseline).unwrap(),
        "the accepted offer opened one cell and moved nothing else anywhere"
    );

    // A second tap yields no proof, so nothing can be applied again.
    assert!(tap_proof(&fresh.record_answer(&fresh_id, CHOSEN)).is_none());
}
