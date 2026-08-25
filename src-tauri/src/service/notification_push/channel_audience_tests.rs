//! A channel surface answers who, not a second whether.
//!
//! The grid column is the whole of whether a level reaches a channel; the
//! channel's audience is the whole of who receives it. These two axes are
//! proved separately here, and the audience is proved to be exactly what was
//! chosen — an empty choice reaches nobody rather than falling back to the
//! inbound admission list that admits senders.

use super::*;
use crate::notification::channels::{self, ChannelHost};
use codeterm_bridge::bridge::manager::OutboundMessage;
use codeterm_types::config::schema::{AgentLevelChoice, AppConfig, PushChannelConfig};
use parking_lot::Mutex;

const CHANNEL: &str = "telegram";
const CHOSEN: &str = "100000001";
const ADMITTED_BUT_UNCHOSEN: &str = "100000002";
const ALSO_ADMITTED: &str = "100000003";

/// A host that carries the channel and records every id a message was addressed
/// to, so "who received it" is an observation rather than an inference.
struct RecordingHost {
    sent: Mutex<Vec<(String, Option<i64>)>>,
}

impl RecordingHost {
    fn carrying() -> Self {
        Self {
            sent: Mutex::new(Vec::new()),
        }
    }

    fn addressees(&self) -> Vec<i64> {
        self.sent
            .lock()
            .iter()
            .filter(|(service, _)| service.as_str() == CHANNEL)
            .filter_map(|(_, user_id)| *user_id)
            .collect()
    }
}

impl ChannelHost for RecordingHost {
    fn service_ready(&self, service: &str) -> bool {
        service == CHANNEL
    }

    fn enqueue(&self, service: &str, message: OutboundMessage) -> Result<(), String> {
        self.sent.lock().push((service.to_string(), message.user_id));
        Ok(())
    }
}

/// Every level's column open for this channel, stated outright rather than
/// inherited from whatever the shipped defaults happen to be.
fn config_with(admitted: &[&str], chosen: &[&str]) -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.integrations.telegram_enabled = true;
    cfg.integrations.telegram_allowed_users =
        admitted.iter().map(|id| id.to_string()).collect();
    cfg.notifications.agent_levels = NotifyLevel::ALL
        .into_iter()
        .map(|level| {
            let mut row = AgentLevelChoice::open(level.as_str());
            row.set(NotifyDestination::Channel(CHANNEL), true);
            row
        })
        .collect();
    cfg.notifications.channels.insert(
        CHANNEL.to_string(),
        PushChannelConfig {
            recipients: chosen.iter().map(|id| id.to_string()).collect(),
            per_scope_limit: 12,
        },
    );
    cfg
}

fn intent() -> PushIntent {
    PushIntent {
        level: Some(NotifyLevel::Failure),
        event_key: Some("release-42".to_string()),
    }
}

fn envelope() -> PushEnvelope {
    PushEnvelope {
        level: NotifyLevel::Failure,
        text: "a rendered push".to_string(),
        notification_id: "notif".to_string(),
    }
}

fn deliver(cfg: &AppConfig, host: &RecordingHost) -> Result<(), String> {
    let plan = plan_push(cfg, &channels::roster(), host, intent());
    deliver_plan(&plan.targets, &plan.refusals, host, &envelope())
}

/// The trap. An audience nobody chose is nobody. The admission list is
/// deliberately populated here, so a fallback to it would turn every admitted
/// sender into a subscriber and this test would fail rather than pass quietly.
#[test]
fn channel_audience_empty_selection_reaches_nobody_with_no_admission_fallback() {
    let cfg = config_with(&[CHOSEN, ADMITTED_BUT_UNCHOSEN, ALSO_ADMITTED], &[]);
    assert_eq!(
        cfg.integrations.telegram_allowed_users.len(),
        3,
        "the admission list is populated, so an empty audience is a choice and not an absence"
    );

    let host = RecordingHost::carrying();
    let plan = plan_push(&cfg, &channels::roster(), &host, intent());
    assert!(
        plan.targets.is_empty(),
        "an empty audience resolves to no target even with the column open"
    );
    assert!(
        plan.refusals.is_empty(),
        "nobody was chosen, so nothing was routed and there is nothing to report"
    );

    assert_eq!(deliver(&cfg, &host), Ok(()));
    assert!(
        host.addressees().is_empty(),
        "nobody was addressed; an empty audience never widens to the admitted ids {:?}",
        cfg.integrations.telegram_allowed_users
    );
}

/// The gate the removed master toggle used to perform, now performed by the
/// column. Both directions are set outright, so neither answer is inherited
/// from the shipped default for an unnamed column.
#[test]
fn channel_audience_column_off_does_not_push_and_column_on_pushes_to_the_selection() {
    let mut closed = config_with(&[CHOSEN, ADMITTED_BUT_UNCHOSEN], &[CHOSEN]);
    for row in &mut closed.notifications.agent_levels {
        row.set(NotifyDestination::Channel(CHANNEL), false);
    }
    assert!(
        !closed
            .notifications
            .agent_levels
            .iter()
            .any(|row| row.allows(NotifyDestination::Channel(CHANNEL))),
        "the column is closed by this fixture, not by an absent key reading as closed"
    );
    let shut = RecordingHost::carrying();
    assert_eq!(deliver(&closed, &shut), Ok(()));
    assert!(
        shut.addressees().is_empty(),
        "a closed column reaches nobody even though an audience is configured"
    );

    let open = config_with(&[CHOSEN, ADMITTED_BUT_UNCHOSEN], &[CHOSEN]);
    let carrying = RecordingHost::carrying();
    assert_eq!(deliver(&open, &carrying), Ok(()));
    assert_eq!(
        carrying.addressees(),
        vec![CHOSEN.parse::<i64>().unwrap()],
        "an open column reaches exactly the chosen audience, not everyone admitted"
    );
}

/// Several admitted users, some chosen. The audience is the selection and only
/// the selection, in the order it was stored.
#[test]
fn channel_audience_stores_exactly_the_selection_never_every_admitted_user() {
    let cfg = config_with(
        &[CHOSEN, ADMITTED_BUT_UNCHOSEN, ALSO_ADMITTED],
        &[CHOSEN, ALSO_ADMITTED],
    );
    let host = RecordingHost::carrying();

    let plan = plan_push(&cfg, &channels::roster(), &host, intent());
    let target = plan
        .targets
        .iter()
        .find(|target| target.service == CHANNEL)
        .expect("the open column resolved this channel");
    assert_eq!(
        target.audience.0,
        vec![CHOSEN.to_string(), ALSO_ADMITTED.to_string()],
        "the resolved audience is the stored selection verbatim"
    );

    assert_eq!(deliver(&cfg, &host), Ok(()));
    assert_eq!(
        host.addressees(),
        vec![
            CHOSEN.parse::<i64>().unwrap(),
            ALSO_ADMITTED.parse::<i64>().unwrap()
        ],
        "the unchosen admitted id {ADMITTED_BUT_UNCHOSEN} received nothing"
    );
}
