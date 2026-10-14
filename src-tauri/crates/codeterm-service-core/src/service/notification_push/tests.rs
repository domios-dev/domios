//! Routing over the push-channel registry, and one member proved to deliver.

use super::*;
use crate::notification::channel_ask::PushMessage;
use crate::notification::channels::{
    self, discord, Audience, ChannelHost, ChannelReadiness, PushBounds, PushChannel,
};
use crate::notification::push::PushContent;
use crate::notification::store::NotificationProvenance;
use codeterm_bridge::bridge::manager::OutboundMessage;
use codeterm_bridge::bridge::traits::OutboundContent;
use codeterm_bridge::services::ServiceDescriptor;
use codeterm_types::config::schema::{AgentLevelChoice, AppConfig, PushChannelConfig};
use parking_lot::Mutex;

/// A host whose readiness the test states outright, so a channel's own answer
/// is what is under test rather than this machine's bridge.
struct FakeHost {
    ready: Vec<&'static str>,
    sent: Mutex<Vec<(String, OutboundMessage)>>,
}

impl FakeHost {
    fn carrying(ready: &[&'static str]) -> Self {
        Self {
            ready: ready.to_vec(),
            sent: Mutex::new(Vec::new()),
        }
    }

    fn texts(&self) -> Vec<(String, Option<i64>, String)> {
        self.sent
            .lock()
            .iter()
            .map(|(service, message)| {
                let text = match &message.content {
                    OutboundContent::Text { text } => text.clone(),
                    other => format!("{other:?}"),
                };
                (service.clone(), message.user_id, text)
            })
            .collect()
    }
}

impl ChannelHost for FakeHost {
    fn service_ready(&self, service: &str) -> bool {
        self.ready.iter().any(|known| *known == service)
    }

    fn enqueue(&self, service: &str, message: OutboundMessage) -> Result<(), String> {
        if !self.service_ready(service) {
            return Err(format!("{service} is not carrying traffic"));
        }
        self.sent.lock().push((service.to_string(), message));
        Ok(())
    }
}

/// A channel whose id core has never seen, registered from a descriptor this
/// test writes. Nothing in `codeterm-types` knows it exists.
static FIXTURE_SERVICE: ServiceDescriptor = ServiceDescriptor {
    id: "a-channel-core-never-heard-of",
    label: "Fixture",
    outbound: true,
};

static FIXTURE_CHANNEL: FixtureChannel = FixtureChannel;

struct FixtureChannel;

impl PushChannel for FixtureChannel {
    fn service(&self) -> &'static str {
        FIXTURE_SERVICE.id
    }

    fn readiness(&self, _cfg: &AppConfig, host: &dyn ChannelHost) -> ChannelReadiness {
        if host.service_ready(FIXTURE_SERVICE.id) {
            ChannelReadiness::Ready(Audience(vec!["someone".to_string()]))
        } else {
            ChannelReadiness::Unavailable("the fixture transport is down".to_string())
        }
    }

    fn per_scope_limit(&self, _cfg: &AppConfig) -> u32 {
        7
    }

    fn bounds(&self) -> PushBounds {
        PushBounds::for_ceiling(1000)
    }

    fn control_available(&self, _cfg: &AppConfig, host: &dyn ChannelHost) -> bool {
        host.service_ready(FIXTURE_SERVICE.id)
    }

    fn deliver(
        &self,
        host: &dyn ChannelHost,
        _audience: &Audience,
        message: &PushMessage,
    ) -> Result<(), String> {
        host.enqueue(
            FIXTURE_SERVICE.id,
            OutboundMessage {
                user_id: None,
                content: OutboundContent::Text {
                    text: message.text.clone(),
                },
                append: false,
                buttons: None,
                edit: None,
                receipt: None,
                route: Default::default(),
            },
        )
    }
}

fn fixture_roster() -> Vec<RegisteredChannel> {
    vec![RegisteredChannel {
        channel: &FIXTURE_CHANNEL,
        service: &FIXTURE_SERVICE,
    }]
}

fn intent(level: NotifyLevel) -> PushIntent {
    PushIntent {
        level: Some(level),
        event_key: Some("release-42".to_string()),
    }
}

/// A push with nothing but a title, and the text a channel will render from it.
fn titled(level: NotifyLevel, title: &str) -> (PushEnvelope, String) {
    let envelope = PushEnvelope {
        content: PushContent {
            level,
            title: title.to_string(),
            body: None,
            provenance: NotificationProvenance::default(),
        },
        choices: Vec::new(),
        record: String::new(),
    };
    let rendered = format!("[{}] {title}", level.label());
    (envelope, rendered)
}

/// Every level routed to every column the real registry resolves. Each channel
/// is named, because a column nobody named reads closed.
fn open_config() -> AppConfig {
    let ids = registered_ids();
    let mut cfg = AppConfig::default();
    cfg.notifications.desktop = true;
    cfg.notifications.agent_levels = NotifyLevel::ALL
        .into_iter()
        .map(|level| AgentLevelChoice::open(level.as_str(), &ids))
        .collect();
    cfg
}

/// The same, for the tests that route against `fixture_roster`. It names the
/// fixture channel and no other: a column pointing at an id the roster in hand
/// cannot resolve is deliberately a reported refusal, not a no-op.
fn fixture_open_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.notifications.desktop = true;
    cfg.notifications.agent_levels = NotifyLevel::ALL
        .into_iter()
        .map(|level| AgentLevelChoice::open(level.as_str(), &[FIXTURE_SERVICE.id]))
        .collect();
    cfg
}

/// The registry's ids. Production never builds a column list — the daemon
/// routes per notification through `plan_push`, and the grid gets its columns
/// from the roster endpoint — so this belongs to the tests that check columns.
fn registered_ids() -> Vec<&'static str> {
    channels::roster()
        .into_iter()
        .map(|entry| entry.id())
        .collect()
}

/// Admitted inbound and chosen as an audience are two different answers, so
/// this fixture states both: the allow list admits the sender, and the
/// channel's own entry is where that id was chosen to receive pushes.
fn discord_opted_in() -> AppConfig {
    let mut cfg = open_config();
    cfg.integrations.discord_enabled = true;
    cfg.integrations.discord_allowed_users = vec!["424242".to_string()];
    cfg.notifications.channels.insert(
        "discord".to_string(),
        PushChannelConfig {
            recipients: vec!["424242".to_string()],
            ..PushChannelConfig::default()
        },
    );
    cfg
}

/* --------------------------------------------------------------------------
 * The four registry properties.
 * ----------------------------------------------------------------------- */

/// A second push channel becomes routable through registry data and its own
/// module. Nothing here edits the destination enum, the grid, or a match arm
/// that spells a channel: Discord routes because the roster carries it and
/// because its own channel entry names an audience.
#[test]
fn channel_registry_second_channel_routes_without_touching_the_enum_or_the_grid() {
    let roster = channels::roster();
    assert!(
        roster.iter().any(|entry| entry.id() == "discord"),
        "the registry carries a second member"
    );

    // The grid's columns are the local surfaces plus whatever the registry
    // resolved. Nobody added a Discord column by hand.
    let ids = registered_ids();
    let columns = NotifyDestination::columns(ids.clone());
    assert_eq!(columns.len(), NotifyDestination::LOCAL.len() + ids.len());
    assert!(columns.iter().any(|column| column.as_str() == "discord"));
    assert!(
        NotifyDestination::LOCAL
            .iter()
            .all(|surface| !surface.is_channel()),
        "core's own column list names no channel at all"
    );

    // Use the service id as the roster key so opting in, not a default, enables the channel.
    let cfg = discord_opted_in();
    let row = cfg
        .notifications
        .agent_levels
        .iter()
        .find(|row| row.level == NotifyLevel::Failure.as_str())
        .expect("the level has a row");
    assert!(
        row.channels.contains_key("discord"),
        "the column is keyed by the roster's id, never by a field core spelled"
    );
    assert!(row.allows(NotifyDestination::Channel("discord")));

    let host = FakeHost::carrying(&["discord"]);
    let plan = plan_push(&cfg, &roster, &host, intent(NotifyLevel::Failure));
    assert!(
        plan.targets
            .iter()
            .any(|target| target.service == "discord"),
        "discord is routable from registry data plus its own audience entry"
    );

    // Exercise audience, readiness, ceiling, and delivery so routability is not mistaken for delivery.
    let (envelope, rendered) = titled(NotifyLevel::Failure, "a rendered push");
    assert_eq!(
        deliver_plan(&plan.targets, &plan.refusals, &host, &envelope),
        Ok(())
    );
    assert_eq!(
        host.texts(),
        vec![("discord".to_string(), None, rendered)],
        "the message left through the second channel's own module"
    );
    assert_eq!(
        plan.per_scope_limit,
        PushChannelConfig::default().per_scope_limit,
        "and its ceiling came from the channel, not from a constant in core"
    );
    assert!(
        discord::CHANNEL.control_available(&cfg, &host),
        "its settings control is offerable once its service can carry traffic"
    );

    // And closing the column closes it, through the same keyed lookup.
    let mut closed = cfg.clone();
    for row in &mut closed.notifications.agent_levels {
        row.set(NotifyDestination::Channel("discord"), false);
    }
    let plan = plan_push(&closed, &roster, &host, intent(NotifyLevel::Failure));
    assert!(plan.targets.is_empty() && plan.refusals.is_empty());
}

/// The desktop surface means exactly what it meant. Its policy reads its own
/// field, and no channel column can move it — nor the stored `bell` field beside
/// it, which routes nothing now but must still survive a channel edit untouched.
#[test]
fn channel_registry_the_local_fields_stay_unchanged_by_channel_columns() {
    let cfg = open_config();
    let desktop_before =
        crate::notification::desktop::DesktopPolicy::from_config(&cfg.notifications);
    assert_eq!(desktop_before.allowed.len(), NotifyLevel::ALL.len());
    assert!(desktop_before.enabled);

    // Close every channel column on every row; the local fields are identical
    // afterwards.
    let mut channels_closed = cfg.clone();
    for row in &mut channels_closed.notifications.agent_levels {
        for id in registered_ids() {
            row.set(NotifyDestination::Channel(id), false);
        }
        assert!(
            row.bell && row.desktop,
            "a channel column moved a local field"
        );
    }
    assert_eq!(
        crate::notification::desktop::DesktopPolicy::from_config(&channels_closed.notifications),
        desktop_before
    );

    // And closing a local field does not touch a channel.
    let mut bell_closed = cfg.clone();
    for row in &mut bell_closed.notifications.agent_levels {
        row.set(NotifyDestination::Bell, false);
        for id in registered_ids() {
            assert!(
                row.allows(NotifyDestination::Channel(id)),
                "closing the bell field moved {id}"
            );
        }
    }
    assert_eq!(
        crate::notification::desktop::DesktopPolicy::from_config(&bell_closed.notifications),
        desktop_before,
        "and it does not move the one local surface that still has a policy"
    );
}

/// Resolving a channel needs a roster entry and nothing else. The proof is a
/// channel whose id exists only in this test: routing, the column, the config
/// lookup and the delivery all work without core naming it.
#[test]
fn channel_registry_resolving_a_channel_never_requires_core_to_name_one() {
    let unknown_to_core = FIXTURE_SERVICE.id;
    let column = NotifyDestination::Channel(unknown_to_core);
    assert!(column.is_channel());
    assert_eq!(column.as_str(), unknown_to_core);
    assert_eq!(
        NotifyDestination::parse_local(unknown_to_core),
        None,
        "core refuses to recognise a channel it has no roster for"
    );

    // A row that has never seen this id answers for it anyway, by key.
    let mut row = AgentLevelChoice::open(NotifyLevel::Verbose.as_str(), &[unknown_to_core]);
    assert!(row.allows(column));
    row.set(column, false);
    assert!(!row.allows(column));
    assert!(row.bell && row.desktop);

    let cfg = fixture_open_config();
    let host = FakeHost::carrying(&[unknown_to_core]);
    let plan = plan_push(&cfg, &fixture_roster(), &host, intent(NotifyLevel::Verbose));
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(plan.targets[0].service, unknown_to_core);
    assert_eq!(
        plan.per_scope_limit, 7,
        "the ceiling comes from the channel, not from a constant in core"
    );

    let (envelope, rendered) = titled(NotifyLevel::Verbose, "a rendered push");
    assert_eq!(
        deliver_plan(&plan.targets, &plan.refusals, &host, &envelope),
        Ok(())
    );
    assert_eq!(
        host.texts(),
        vec![(unknown_to_core.to_string(), None, rendered)]
    );
}

/// A channel that cannot carry the message says so. Unknown ids are refused by
/// name, an opted-in channel with no transport becomes a reported failure, and
/// only a channel nobody opted into stays silent.
#[test]
fn channel_registry_unknown_or_unavailable_channel_degrades_honestly() {
    let roster = channels::roster();
    let unknown = channels::resolve_in(&roster, "a-service-nobody-registered");
    assert!(
        unknown
            .err()
            .is_some_and(|reason| reason.contains("a-service-nobody-registered")),
        "an unknown channel is refused by name, never resolved to nothing"
    );
    assert!(channels::resolve_in(&roster, "discord").is_ok());

    // A stored column naming a channel this build cannot resolve is reported,
    // not skipped: the user turned that column on deliberately.
    let mut stale = open_config();
    for row in &mut stale.notifications.agent_levels {
        row.set(
            NotifyDestination::Channel("a-service-nobody-registered"),
            true,
        );
    }
    let stale_plan = plan_push(
        &stale,
        &roster,
        &FakeHost::carrying(&[]),
        intent(NotifyLevel::Verbose),
    );
    assert!(
        stale_plan
            .refusals
            .iter()
            .any(|refusal| refusal.service == "a-service-nobody-registered"),
        "a column pointing at nothing is named, never silently ignored"
    );

    // Opted in, no transport: the level still opens a record, and delivery
    // reports why nobody got it.
    let cfg = discord_opted_in();
    let dead = FakeHost::carrying(&[]);
    let plan = plan_push(&cfg, &roster, &dead, intent(NotifyLevel::Failure));
    assert!(plan.targets.is_empty());
    assert_eq!(plan.refusals.len(), 1);
    assert_eq!(plan.refusals[0].service, "discord");
    assert!(
        plan.intent.level.is_some(),
        "a refusal keeps the push leg so the record can be marked failed"
    );
    let (envelope, _) = titled(NotifyLevel::Failure, "text");
    let outcome = deliver_plan(&plan.targets, &plan.refusals, &dead, &envelope);
    assert!(
        outcome.is_err_and(|reason| reason.contains("discord")),
        "an unavailable channel is a reported failure, not a silent drop"
    );

    // Nobody opted in: nothing was routed, so there is nothing to report.
    let untouched = open_config();
    let plan = plan_push(&untouched, &roster, &dead, intent(NotifyLevel::Failure));
    assert!(plan.targets.is_empty() && plan.refusals.is_empty());
    assert!(
        plan.intent.level.is_none(),
        "an install that never opted in takes exactly the pre-existing path"
    );
}

/* --------------------------------------------------------------------------
 * Routing regressions the grid already owed, now asked of the registry.
 * ----------------------------------------------------------------------- */

fn column_routing(level: NotifyLevel, routed: bool) -> AppConfig {
    let mut cfg = fixture_open_config();
    for row in &mut cfg.notifications.agent_levels {
        if NotifyLevel::parse(&row.level) == Some(level) {
            row.set(NotifyDestination::Channel(FIXTURE_SERVICE.id), routed);
        }
    }
    cfg
}

#[test]
fn notification_settings_channel_column_gates_only_its_own_level() {
    let cfg = column_routing(NotifyLevel::Verbose, false);
    let host = FakeHost::carrying(&[FIXTURE_SERVICE.id]);
    let roster = fixture_roster();
    assert!(
        plan_push(&cfg, &roster, &host, intent(NotifyLevel::Verbose))
            .targets
            .is_empty()
    );
    assert!(
        !plan_push(&cfg, &roster, &host, intent(NotifyLevel::ActionNeeded))
            .targets
            .is_empty()
    );
    assert!(
        !plan_push(&cfg, &roster, &host, intent(NotifyLevel::Failure))
            .targets
            .is_empty()
    );
}

/// The shipped grid is the absent grid, and it routes no level to any channel:
/// push is opt-in, even to a channel that is carrying and has an audience.
#[test]
fn notification_settings_shipped_grid_routes_no_level_to_any_channel() {
    let mut cfg = AppConfig::default();
    cfg.notifications.agent_levels = codeterm_types::config::schema::default_agent_levels();
    let host = FakeHost::carrying(&[FIXTURE_SERVICE.id]);
    let roster = fixture_roster();
    for level in NotifyLevel::ALL {
        let plan = plan_push(&cfg, &roster, &host, intent(level));
        assert!(
            plan.targets.is_empty() && plan.refusals.is_empty(),
            "{} reaches no channel until someone opts that column in",
            level.as_str()
        );
    }
    assert!(
        plan_push(&cfg, &roster, &host, PushIntent::default())
            .targets
            .is_empty(),
        "naming no level never reached a channel"
    );
}

#[test]
fn notification_settings_level_with_no_readable_row_is_refused() {
    let mut cfg = AppConfig::default();
    cfg.notifications.agent_levels =
        vec![AgentLevelChoice::open("milestone", &[FIXTURE_SERVICE.id])];
    let host = FakeHost::carrying(&[FIXTURE_SERVICE.id]);
    let roster = fixture_roster();
    for level in NotifyLevel::ALL {
        let plan = plan_push(&cfg, &roster, &host, intent(level));
        assert!(plan.targets.is_empty() && plan.refusals.is_empty());
    }
}

/// The whole outbound leg of spending a question on the one channel that can
/// rewrite: the placement it reports on delivery, and the rewrite it enqueues
/// for that exact message.
#[test]
fn spent_question_the_telegram_channel_rewrites_the_message_it_placed() {
    let host = FakeHost::carrying(&["telegram"]);
    let asked = PushMessage {
        text: "[action needed] Deploy?".to_string(),
        choices: vec![
            crate::notification::channel_ask::ChannelChoice {
                label: "Ship it".to_string(),
                token: "na:record-1:0".to_string(),
            },
            crate::notification::channel_ask::ChannelChoice {
                label: "Hold".to_string(),
                token: "na:record-1:1".to_string(),
            },
        ],
        record: "record-1".to_string(),
        origin_tab: None,
    };

    channels::telegram::CHANNEL
        .deliver(&host, &Audience(vec!["4242".to_string()]), &asked)
        .expect("a ready telegram channel carries the question");

    let placed = host.sent.lock().pop().expect("the question was enqueued");
    assert!(
        placed.1.receipt.is_some(),
        "a message that draws options is tagged so its placement can come back"
    );
    let receipt = placed.1.receipt.clone().expect("tagged");
    assert_eq!(receipt.service, "telegram");
    assert_eq!(
        receipt.tag, "record-1",
        "the placement names the record the channel was handed"
    );

    // What the bridge reports back for that send, spelled by the transport.
    let locator = codeterm_bridge::bridge::telegram::locator(4242, 77);
    let spent = PushMessage {
        text: "[action needed] Deploy?\n\nAnswered: Ship it".to_string(),
        choices: Vec::new(),
        record: "record-1".to_string(),
        origin_tab: None,
    };
    channels::telegram::CHANNEL
        .spend(&host, &locator, &spent)
        .expect("the channel rewrites the message it placed");

    let rewrite = host.sent.lock().pop().expect("the rewrite was enqueued");
    assert_eq!(rewrite.0, "telegram");
    assert_eq!(rewrite.1.user_id, Some(4242));
    let edit = rewrite.1.edit.as_ref().expect("a rewrite names a message");
    assert_eq!(
        edit.message_id, 77,
        "the rewrite addresses the message the placement reported"
    );
    assert_eq!(
        rewrite.1.buttons.as_ref().map(|kb| kb.len()),
        Some(0),
        "a spent question restates an empty keyboard rather than omitting it"
    );
    match &rewrite.1.content {
        OutboundContent::Text { text } => assert!(
            text.contains("Ship it"),
            "the chosen label reaches the rewritten message"
        ),
        other => panic!("expected text, got {other:?}"),
    }
}

/// A locator this channel did not write is refused rather than addressed as
/// one of its own messages.
#[test]
fn spent_question_a_foreign_locator_is_refused_rather_than_guessed() {
    let host = FakeHost::carrying(&["telegram"]);
    let refusal = channels::telegram::CHANNEL
        .spend(&host, "not-a-telegram-message", &PushMessage::text("spent"))
        .expect_err("an unreadable locator cannot be addressed");
    assert!(refusal.contains("telegram"));
    assert!(host.sent.lock().is_empty(), "nothing was enqueued");
}

/// A channel with no rewrite of its own says so. Nothing above it may read a
/// missing capability as a rewrite that happened.
#[test]
fn spent_question_a_channel_that_cannot_rewrite_degrades_honestly() {
    let host = FakeHost::carrying(&["discord", FIXTURE_SERVICE.id]);

    let discord_refusal = discord::CHANNEL
        .spend(&host, "anything", &PushMessage::text("spent"))
        .expect_err("this build carries no discord rewrite");
    assert!(discord_refusal.contains("discord"));

    let fixture_refusal = FIXTURE_CHANNEL
        .spend(&host, "anything", &PushMessage::text("spent"))
        .expect_err("a channel that never implemented one cannot rewrite");
    assert!(fixture_refusal.contains(FIXTURE_SERVICE.id));

    assert!(
        host.sent.lock().is_empty(),
        "a channel that cannot rewrite enqueues nothing"
    );
}
