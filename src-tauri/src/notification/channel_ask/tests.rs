//! An escalation asks where it lands, and an agent may offer to be louder.
//!
//! Every test here drives production seams: the store admits the push, the
//! registry routes it, a registered channel carries the options, the tapped
//! token is resolved against the record, and the answer is owed to the tab the
//! record captured when the question was sent.

use codeterm_bridge::bridge::manager::OutboundMessage;
use codeterm_bridge::bridge::traits::OutboundContent;
use codeterm_types::config::schema::{AgentLevelChoice, AppConfig, PushChannelConfig};
use codeterm_types::notification::{
    adopt_caller_actions, answer_actions, validate_answer_labels, CallerAction, NotifyDestination,
    NotifyLevel,
};
use parking_lot::Mutex;

use super::*;
use crate::notification::answer::AnswerAdmission;
use crate::notification::answer_delivery::AnswerDeliveryPlan;
use crate::notification::authority::{
    self, admit_unattended_change, adopt_offer, apply, tap_proof, SurfaceOffer, SurfaceTarget,
};
use crate::notification::channels::{self, telegram, ChannelHost, PushChannel};
use crate::notification::push;
use crate::notification::store::{
    Ask, NotificationProvenance, NotificationSeverity, NotificationStore, PushRequest,
};
use crate::service::notification::{PushEnvelope, PushIntent};
use crate::service::notification_push::{deliver_plan, plan_push};

const ASKING_TAB: &str = "tab_the_one_that_asked";
const CHOSEN: &str = "Rebase";
const DECLINED: &str = "Merge";
const RECIPIENT: &str = "100000001";

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

/// One registered channel, addressed through the registry rather than by name,
/// so this test spells no provider id.
fn a_registered_channel() -> &'static str {
    channels::telegram::CHANNEL.service()
}

fn choice_set(labels: &[&str]) -> Vec<CallerAction> {
    answer_actions(&labels.iter().map(|l| l.to_string()).collect::<Vec<_>>())
        .expect("distinct non-empty labels are a legal choice set")
}

fn ask(labels: &[&str], offer: Option<SurfaceOffer>) -> Ask {
    Ask {
        actions: Some(adopt_caller_actions(choice_set(labels))),
        offer,
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

/// A host that is carrying traffic, recording whatever a channel handed it.
struct RecordingHost {
    sent: Mutex<Vec<OutboundMessage>>,
}

impl RecordingHost {
    fn new() -> Self {
        Self {
            sent: Mutex::new(Vec::new()),
        }
    }
}

impl ChannelHost for RecordingHost {
    fn service_ready(&self, _service: &str) -> bool {
        true
    }

    fn enqueue(&self, _service: &str, message: OutboundMessage) -> Result<(), String> {
        self.sent.lock().push(message);
        Ok(())
    }
}

/// One channel routed at every level and given an audience. Whether a level
/// reaches the channel is the row's column - the entry answers only who.
fn opted_in_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    let channel = a_registered_channel();
    cfg.notifications.agent_levels = NotifyLevel::ALL
        .into_iter()
        .map(|level| AgentLevelChoice::open(level.as_str(), &[channel]))
        .collect();
    cfg.notifications.channels.insert(
        channel.to_string(),
        PushChannelConfig {
            recipients: vec![RECIPIENT.to_string()],
            per_scope_limit: 8,
        },
    );
    cfg
}

/// A question routed to a push channel carries its options, and the option that
/// comes back reaches the tab that asked — through the answer slot captured at
/// send time, not through anything this path invented.
#[test]
fn channel_ask_options_ride_the_channel_and_the_tapped_label_returns_to_the_asking_tab() {
    let mut store = NotificationStore::new(10);
    let id = asked_question(&mut store, ask(&[CHOSEN, DECLINED], None));

    // What the daemon hands a channel for this record.
    let message = PushMessage {
        text: "rendered".to_string(),
        choices: store.offered_choices_for_channel(&id),
        record: id.clone(),
    };
    assert_eq!(
        message
            .choices
            .iter()
            .map(|choice| choice.label.as_str())
            .collect::<Vec<_>>(),
        vec![CHOSEN, DECLINED],
        "every offered choice reaches the channel, in the order it was offered"
    );
    assert!(
        message
            .choices
            .iter()
            .all(|choice| !choice.token.is_empty()),
        "each option carries the token that names it back"
    );

    // The registry's own routing, and a registered channel carrying it.
    let host = RecordingHost::new();
    let cfg = opted_in_config();
    let roster = channels::roster();
    let plan = plan_push(
        &cfg,
        &roster,
        &host,
        PushIntent {
            level: Some(NotifyLevel::ActionNeeded),
            event_key: None,
        },
    );
    assert_eq!(plan.targets.len(), 1, "exactly the opted-in channel routed");
    let envelope = PushEnvelope {
        content: push::PushContent {
            level: NotifyLevel::ActionNeeded,
            title: "rendered".to_string(),
            body: None,
            provenance: NotificationProvenance::default(),
        },
        choices: message.choices.clone(),
        record: message.record.clone(),
    };
    let carried_text = envelope.message_for(telegram::CHANNEL.bounds()).text;
    deliver_plan(&plan.targets, &plan.refusals, &host, &envelope)
        .expect("a carrying channel accepts the message");

    let carried = host.sent.lock().clone();
    assert_eq!(carried.len(), 1, "one send per recipient");
    let buttons = carried[0]
        .buttons
        .clone()
        .expect("a question reaches the channel as options, not as prose");
    let carried_labels: Vec<String> = buttons
        .iter()
        .flatten()
        .map(|button| button.text.clone())
        .collect();
    let carried_tokens: Vec<String> = buttons
        .iter()
        .flatten()
        .filter_map(|button| button.data.clone())
        .collect();
    assert_eq!(
        carried_labels,
        vec![CHOSEN.to_string(), DECLINED.to_string()],
        "the options a channel drew are the options the record offers"
    );
    assert_eq!(
        carried_tokens,
        message
            .choices
            .iter()
            .map(|choice| choice.token.clone())
            .collect::<Vec<_>>(),
        "what comes back is the token the daemon minted, never the visible text"
    );
    let OutboundContent::Text { text } = &carried[0].content else {
        panic!("a push is text plus its options");
    };
    assert_eq!(
        text, &carried_text,
        "the channel carried the text it rendered at its own bound"
    );

    // The far end: the first option is tapped and its token comes home.
    let tapped = decode(&carried_tokens[0]).expect("the carried token reads back");
    assert_eq!(tapped.notification_id, id, "the token names its own record");
    let label = store
        .offered_choice(&tapped.notification_id, tapped.index)
        .expect("the position resolves against the record that offered it");
    assert_eq!(label, CHOSEN, "the tapped label round-tripped");

    // The existing answer path, and nothing beside it.
    assert_eq!(
        store.record_answer(&id, &label),
        AnswerAdmission::Recorded(CHOSEN.to_string())
    );
    match store.answer_delivery_plan(&id) {
        AnswerDeliveryPlan::Owed(owed) => {
            assert_eq!(
                owed.to_tab, ASKING_TAB,
                "the answer is owed to the tab that asked, captured at send time"
            );
            assert_eq!(owed.label, CHOSEN);
        }
        other => panic!("a tapped answer is owed to the asking tab, got {other:?}"),
    }

    // A forged position names nothing this record ever offered, and ordinary
    // channel traffic is never mistaken for an answer.
    assert_eq!(store.offered_choice(&id, 99), None);
    assert_eq!(decode("hello there"), None);
    assert_eq!(decode(""), None);
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

    // The config seam an agent's own CLI reaches: a keyed write is refused,
    // and a whole-object write from inside a tab is refused too.
    let mut surface_partial = serde_json::Map::new();
    surface_partial.insert(
        authority::SURFACE_ROOT.to_string(),
        serde_json::json!({"desktop": true}),
    );
    let empty = serde_json::Map::new();
    assert!(
        crate::api::config_routes::admit_config_write(None, Some("notifications.desktop"), &empty)
            .is_err(),
        "the keyed write is the shape an agent's CLI makes"
    );
    assert!(
        crate::api::config_routes::admit_config_write(Some(ASKING_TAB), None, &surface_partial)
            .is_err(),
        "a whole-object write from inside a tab is still nobody tapping anything"
    );
    assert!(
        crate::api::config_routes::admit_config_write(None, Some("terminal.font_size"), &empty)
            .is_ok(),
        "the rule guards the surface subtree and nothing else"
    );

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

    // What a tap may change, exactly: ONE cell of the grid — the offered
    // column on the row of the level the user was actually shown — plus, for
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
    // The proportionality criterion, and the reason it is only one level: a tap
    // on one question must not route a kind of notification the user was never
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
    let service = a_registered_channel().to_string();
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

    // And it may only be made by a notification that HAS a level, because the
    // level is the row the tap opens. An offer that could never apply is
    assert_eq!(
        crate::api::notification_routes::offer_requires_level(&service, Some(ASKED_AT)),
        Ok(ASKED_AT),
        "a levelled notification names the row its tap would open"
    );
    assert!(
        crate::api::notification_routes::offer_requires_level(&service, None).is_err(),
        "an unlevelled notification has no row, so it cannot offer one"
    );

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

/// The choice set of an ask-notification is vetted by the one shared
/// validator, on the offer path exactly as on every other. Nothing here
/// restates what a refusable label is.
#[test]
fn channel_ask_labels_are_refused_by_the_existing_validation() {
    for unusable in [
        vec!["Rebase".to_string(), "  ".to_string()],
        vec!["Rebase".to_string(), String::new()],
        vec!["Rebase".to_string(), "rebase".to_string()],
        vec!["Rebase".to_string(), " REBASE ".to_string()],
    ] {
        let built = answer_actions(&unusable);
        assert!(
            built.is_err(),
            "{unusable:?} is not a choice set a user could tell apart"
        );
        // The very same refusal the shared validator produces for the same
        // input: this path reuses it rather than carrying a copy of the rule.
        let vetted: Vec<CallerAction> = unusable
            .iter()
            .map(|label| CallerAction::Answer {
                label: label.trim().to_string(),
            })
            .collect();
        assert_eq!(
            built.unwrap_err(),
            validate_answer_labels(&vetted).expect_err("the shared validator refuses the same set"),
            "the offer path refuses with the shared validator's own answer"
        );
    }

    // A legal set stays legal, and its offered positions are what a channel
    // addresses — so a token cannot name a label that was never offered.
    let legal = choice_set(&[CHOSEN, DECLINED]);
    assert!(validate_answer_labels(&legal).is_ok());
    assert_eq!(offered_labels(&legal), vec![CHOSEN, DECLINED]);
    assert_eq!(label_at(&legal, 1), Some(DECLINED));
    assert_eq!(label_at(&legal, 2), None);

    // And an offer cannot ride a notification that offers no choice at all.
    assert!(adopt_offer(SurfaceTarget::Desktop, CHOSEN, &[] as &[CallerAction]).is_err());
}
