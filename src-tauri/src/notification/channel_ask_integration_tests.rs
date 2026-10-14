//! An escalation asks where it lands, and an agent may offer to be louder.
//!
//! Every test here drives production seams: the store admits the push, the
//! registry routes it, a registered channel carries the options, the tapped
//! token is resolved against the record, and the answer is owed to the tab the
//! record captured when the question was sent.

use codeterm_bridge::bridge::manager::OutboundMessage;
use codeterm_bridge::bridge::traits::OutboundContent;
use codeterm_notification_core::contract::{
    adopt_caller_actions, answer_actions, CallerAction, NotifyLevel,
};
use codeterm_types::config::schema::{AgentLevelChoice, AppConfig, PushChannelConfig};
use parking_lot::Mutex;

use super::channel_ask::*;
use crate::notification::answer::AnswerAdmission;
use crate::notification::answer_delivery::AnswerDeliveryPlan;
use crate::notification::authority::{self, SurfaceOffer};
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
        origin_tab: None,
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

#[test]
fn channel_ask_authority_refuses_every_unattended_surface_change() {
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
}

#[test]
fn channel_ask_offer_requires_a_level() {
    let service = a_registered_channel().to_string();
    // An offer that could never apply is refused when it is made, not accepted and left silently inert.
    assert_eq!(
        crate::api::notification_routes::offer_requires_level(&service, Some(ASKED_AT)),
        Ok(ASKED_AT),
        "a levelled notification names the row its tap would open"
    );
    assert!(
        crate::api::notification_routes::offer_requires_level(&service, None).is_err(),
        "an unlevelled notification has no row, so it cannot offer one"
    );
}
