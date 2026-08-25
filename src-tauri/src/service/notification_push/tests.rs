//! Routing over the push-channel registry, and one member proved to deliver.

use super::*;
use crate::notification::channels::{
    self, discord, Audience, ChannelHost, ChannelReadiness, PushChannel,
};
use codeterm_bridge::bridge::manager::OutboundMessage;
use crate::notification::channel_ask::PushMessage;
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
    channels::roster().into_iter().map(|entry| entry.id()).collect()
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

    // The row routes to Discord through a keyed lookup on the id the roster
    // carries. Nothing named a Discord field: the key is the service id, and it
    // is on because someone opted in, never because it defaulted open.
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
        plan.targets.iter().any(|target| target.service == "discord"),
        "discord is routable from registry data plus its own audience entry"
    );

    // Routable is not the claim; carrying is. Every function of the second
    // channel's module runs here — its audience, its readiness, its ceiling,
    // its delivery — so the module that is this design's evidence is exercised
    // rather than merely compiled.
    let envelope = PushEnvelope {
        level: NotifyLevel::Failure,
        message: PushMessage::text("a rendered push"),
        notification_id: "notif".to_string(),
    };
    assert_eq!(
        deliver_plan(&plan.targets, &plan.refusals, &host, &envelope),
        Ok(())
    );
    assert_eq!(
        host.texts(),
        vec![(
            "discord".to_string(),
            None,
            "a rendered push".to_string()
        )],
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

/// Bell and desktop are local surfaces and mean exactly what they meant. Their
/// policies read their own fields, and no channel column can move them.
#[test]
fn channel_registry_bell_and_desktop_stay_unchanged_local_surfaces() {
    let cfg = open_config();
    let bell_before = crate::notification::bell::BellPolicy::from_config(&cfg.notifications);
    let desktop_before =
        crate::notification::desktop::DesktopPolicy::from_config(&cfg.notifications);
    assert_eq!(bell_before, crate::notification::bell::BellPolicy::open());
    assert_eq!(desktop_before.allowed.len(), NotifyLevel::ALL.len());
    assert!(desktop_before.enabled);

    // Close every channel column on every row; the two local surfaces are
    // byte-identical afterwards.
    let mut channels_closed = cfg.clone();
    for row in &mut channels_closed.notifications.agent_levels {
        for id in registered_ids() {
            row.set(NotifyDestination::Channel(id), false);
        }
        assert!(row.bell && row.desktop, "a channel column moved a surface");
    }
    assert_eq!(
        crate::notification::bell::BellPolicy::from_config(&channels_closed.notifications),
        bell_before
    );
    assert_eq!(
        crate::notification::desktop::DesktopPolicy::from_config(&channels_closed.notifications),
        desktop_before
    );

    // And closing a surface does not touch a channel.
    let mut bell_closed = cfg.clone();
    for row in &mut bell_closed.notifications.agent_levels {
        row.set(NotifyDestination::Bell, false);
        for id in registered_ids() {
            assert!(
                row.allows(NotifyDestination::Channel(id)),
                "closing the bell moved {id}"
            );
        }
    }
    assert!(
        crate::notification::bell::BellPolicy::from_config(&bell_closed.notifications)
            .allowed
            .is_empty()
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
        column.local_label(),
        None,
        "core has no caption for a channel; the roster entry owns that"
    );
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

    let envelope = PushEnvelope {
        level: NotifyLevel::Verbose,
        message: PushMessage::text("a rendered push"),
        notification_id: "notif".to_string(),
    };
    assert_eq!(
        deliver_plan(&plan.targets, &plan.refusals, &host, &envelope),
        Ok(())
    );
    assert_eq!(
        host.texts(),
        vec![(
            unknown_to_core.to_string(),
            None,
            "a rendered push".to_string()
        )]
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
        row.set(NotifyDestination::Channel("a-service-nobody-registered"), true);
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
    let envelope = PushEnvelope {
        level: NotifyLevel::Failure,
        message: PushMessage::text("text"),
        notification_id: "notif".to_string(),
    };
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
