//! `notify_defaults` — what a fresh install does before anyone opens the panel.
//!
//! One claim, held at every level: the record is kept and nothing interrupts.
//! No toast, no push, no sound. Every destination that can refuse is opt-in, and
//! opting into one still works — the change is to the default, never to the
//! mechanism. The panel is not among them: it is the log, and takes every record.

use codeterm_bridge::bridge::manager::OutboundMessage;
use codeterm_bridge::bridge::traits::OutboundContent;
use codeterm_bridge::services::ServiceDescriptor;
use codeterm_types::config::schema::{default_agent_levels, AppConfig, NotificationConfig};
use codeterm_types::notification::{NotifyDestination, NotifyLevel};
use parking_lot::Mutex;

use crate::notification::channels::{
    Audience, ChannelHost, ChannelReadiness, PushBounds, PushChannel, RegisteredChannel,
};
use crate::notification::desktop::{self, DesktopPolicy};
use crate::notification::push::PushContent;
use crate::notification::store::NotificationProvenance;
use crate::service::notification::{PushEnvelope, PushIntent};
use crate::service::notification_push::{deliver_plan, plan_push};

/// A channel id core has never heard of, so nothing below can pass by knowing
/// a real channel's name.
static FIXTURE_SERVICE: ServiceDescriptor = ServiceDescriptor {
    id: "a-channel-notify-defaults-invented",
    label: "Fixture",
    outbound: true,
};

static FIXTURE_CHANNEL: FixtureChannel = FixtureChannel;

/// Always opted in and always carrying. That is the point: when a fresh config
/// reaches it with nothing, the grid column is the only thing that can have
/// closed it — never an audience that was empty anyway.
struct FixtureChannel;

impl PushChannel for FixtureChannel {
    fn service(&self) -> &'static str {
        FIXTURE_SERVICE.id
    }

    fn readiness(&self, _cfg: &AppConfig, _host: &dyn ChannelHost) -> ChannelReadiness {
        ChannelReadiness::Ready(Audience(vec!["someone".to_string()]))
    }

    fn per_scope_limit(&self, _cfg: &AppConfig) -> u32 {
        7
    }

    fn bounds(&self) -> PushBounds {
        PushBounds::for_ceiling(1000)
    }

    fn control_available(&self, _cfg: &AppConfig, _host: &dyn ChannelHost) -> bool {
        true
    }

    fn deliver(
        &self,
        host: &dyn ChannelHost,
        _audience: &Audience,
        message: &crate::notification::channel_ask::PushMessage,
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

/// Records what actually left through a channel's own transport, so "it
/// routed" is a delivered message rather than a plan that looked right.
struct RecordingHost {
    sent: Mutex<Vec<String>>,
}

impl RecordingHost {
    fn carrying_everything() -> Self {
        Self {
            sent: Mutex::new(Vec::new()),
        }
    }

    fn delivered(&self) -> Vec<String> {
        self.sent.lock().clone()
    }
}

impl ChannelHost for RecordingHost {
    fn service_ready(&self, _service: &str) -> bool {
        true
    }

    fn enqueue(&self, service: &str, message: OutboundMessage) -> Result<(), String> {
        let text = match &message.content {
            OutboundContent::Text { text } => text.clone(),
            other => format!("{other:?}"),
        };
        self.sent.lock().push(format!("{service}:{text}"));
        Ok(())
    }
}

fn intent(level: NotifyLevel) -> PushIntent {
    PushIntent {
        level: Some(level),
        event_key: None,
    }
}

fn columns() -> Vec<NotifyDestination<'static>> {
    NotifyDestination::columns([FIXTURE_SERVICE.id])
}

/// The destinations a row can actually be routed away from. `Bell` is not one:
/// it is a name core still knows, so an agent offering to switch it on can be
/// refused by name, but it stopped being a column a level answers.
fn interrupting_columns() -> Vec<NotifyDestination<'static>> {
    columns()
        .into_iter()
        .filter(|column| *column != NotifyDestination::Bell)
        .collect()
}

/// The whole task in one test: a config nobody has touched interrupts nowhere,
/// at every level, while keeping every record. Before the default changed this
/// failed on the very first assertion, because every column of every row shipped
/// open.
#[test]
fn notify_defaults_a_fresh_config_interrupts_nowhere_at_every_level() {
    let cfg = AppConfig::default();
    let notifications = &cfg.notifications;
    let desktop_policy = DesktopPolicy::from_config(notifications);
    let roster = fixture_roster();
    let host = RecordingHost::carrying_everything();

    assert!(!notifications.desktop, "the desktop surface is opt-in");
    assert!(!notifications.sound, "a fresh install makes no sound");
    assert!(
        notifications.channels.is_empty(),
        "a fresh install names no push audience"
    );
    assert!(
        notifications.show_bell,
        "the surface the records land on is still drawn"
    );

    for level in NotifyLevel::ALL {
        let wire = level.as_str();
        let row = notifications
            .agent_levels
            .iter()
            .find(|row| NotifyLevel::parse(&row.level) == Some(level))
            .unwrap_or_else(|| panic!("{wire} has a row"));

        assert!(
            !row.allows(NotifyDestination::Desktop),
            "{wire} must not be routed to the desktop"
        );
        assert!(
            desktop::admit(Some(level), &desktop_policy)
                .pass()
                .is_none(),
            "{wire} must not raise a toast"
        );

        assert!(
            !row.allows(NotifyDestination::Channel(FIXTURE_SERVICE.id)),
            "{wire} must not be routed to a push channel"
        );
        let plan = plan_push(&cfg, &roster, &host, intent(level));
        assert!(
            plan.targets.is_empty() && plan.refusals.is_empty(),
            "{wire} must reach no channel, and have nothing to report either"
        );

        assert!(
            row.reaches_nothing(&interrupting_columns()),
            "{wire} interrupts nowhere on a config nobody touched"
        );
    }

    assert_eq!(
        FIXTURE_CHANNEL.readiness(&cfg, &host),
        ChannelReadiness::Ready(Audience(vec!["someone".to_string()])),
        "the closed column is what stopped the push, not a channel with nobody to tell"
    );
    assert!(
        host.delivered().is_empty(),
        "nothing left the machine on a config nobody touched"
    );
}

/// Pins the absent-column read to the meaning it has NOW: a column a stored row
/// never named is OFF. It no longer means "everything the surrounding switches
/// allowed before the grid existed", which is what the old default promised and
/// what this test would have failed on.
#[test]
fn notify_defaults_an_unnamed_column_reads_closed_rather_than_pre_grid_open() {
    let partial: NotificationConfig = toml::from_str(
        r#"
[[agent_levels]]
level = "verbose"
"#,
    )
    .expect("a row that names only its level parses");
    let row = &partial.agent_levels[0];
    assert!(!row.bell, "an unnamed bell field is off, as it always was");
    assert!(!row.desktop, "an unnamed desktop column is off");
    assert!(
        !row.allows(NotifyDestination::Channel(FIXTURE_SERVICE.id)),
        "an unnamed channel column is off, so a channel the registry gains arrives off"
    );
    assert!(
        row.reaches_nothing(&columns()),
        "a level that named nothing falls through to nothing, never to a default"
    );

    // A default, not a gate: the column the row did name keeps what it named.
    let named: NotificationConfig = toml::from_str(
        r#"
[[agent_levels]]
level = "failure"
desktop = true
"#,
    )
    .expect("a row that names one column parses");
    let row = &named.agent_levels[0];
    assert!(
        row.desktop,
        "the named column keeps the answer it was given"
    );
    assert!(!row.bell, "and the fields it left unnamed stay off");

    // An absent grid is a different thing from a grid that named no column: it
    // is the shipped default, which interrupts nowhere while keeping every record.
    let absent: NotificationConfig = toml::from_str("").expect("an empty config parses");
    assert_eq!(absent.agent_levels, default_agent_levels());
    for row in &absent.agent_levels {
        assert!(!row.desktop, "{} raises no toast", row.level);
        assert!(row.reaches_nothing(&interrupting_columns()));
    }
}

/// The change is to the default, not to the mechanism. A destination someone
/// switched on still routes, and the proof is the message arriving at the
/// channel's own transport rather than a plan that merely looked right.
#[test]
fn notify_defaults_an_explicitly_enabled_destination_still_routes() {
    let mut cfg = AppConfig::default();
    cfg.notifications.desktop = true;
    for row in &mut cfg.notifications.agent_levels {
        row.set(NotifyDestination::Desktop, true);
        row.set(NotifyDestination::Channel(FIXTURE_SERVICE.id), true);
    }

    let desktop_policy = DesktopPolicy::from_config(&cfg.notifications);
    let roster = fixture_roster();
    let host = RecordingHost::carrying_everything();
    let mut expected = Vec::new();

    for level in NotifyLevel::ALL {
        let wire = level.as_str();
        assert!(
            desktop::admit(Some(level), &desktop_policy)
                .pass()
                .is_some(),
            "{wire} was switched on, so it reaches the desktop sink"
        );

        let plan = plan_push(&cfg, &roster, &host, intent(level));
        assert_eq!(plan.targets.len(), 1, "{wire} resolved its one channel");
        assert_eq!(plan.targets[0].service, FIXTURE_SERVICE.id);

        let text = format!("{wire} left the machine");
        let envelope = PushEnvelope {
            content: PushContent {
                level,
                title: text.clone(),
                body: None,
                provenance: NotificationProvenance::default(),
            },
            choices: Vec::new(),
            record: String::new(),
        };
        assert_eq!(
            deliver_plan(&plan.targets, &plan.refusals, &host, &envelope),
            Ok(())
        );
        expected.push(format!("{}:[{}] {text}", FIXTURE_SERVICE.id, level.label()));
    }

    assert_eq!(
        host.delivered(),
        expected,
        "every switched-on level arrived through the channel's own transport"
    );

    // And one column switched on switches nothing else on.
    let mut desktop_only = AppConfig::default();
    for row in &mut desktop_only.notifications.agent_levels {
        row.set(NotifyDestination::Desktop, true);
    }
    let untouched = RecordingHost::carrying_everything();
    for level in NotifyLevel::ALL {
        let plan = plan_push(&desktop_only, &roster, &untouched, intent(level));
        assert!(
            plan.targets.is_empty() && plan.refusals.is_empty(),
            "opting {} into the desktop opted it into no channel",
            level.as_str()
        );
    }
    assert!(untouched.delivered().is_empty());
}

/// Sound is presentation riding a record's arrival, not a column of the grid, so
/// "off at every level" is one switch that starts off and that no row can
/// reach. It stays reachable by hand — off is the default, not a removal.
#[test]
fn notify_defaults_sound_is_off_at_every_level() {
    let shipped = NotificationConfig::default();
    assert!(!shipped.sound, "a fresh install makes no sound");
    assert!(
        !toml::from_str::<NotificationConfig>("")
            .expect("an empty config parses")
            .sound,
        "and neither does a config file that never mentioned it"
    );

    // Every column of every level turned on — including one named `sound` —
    // leaves the switch off, because sound is not something a row can route to.
    let mut every_column_on = NotificationConfig::default();
    for row in &mut every_column_on.agent_levels {
        for column in NotifyDestination::columns(["sound", FIXTURE_SERVICE.id]) {
            row.set(column, true);
        }
    }
    assert!(
        !every_column_on.sound,
        "no cell of the grid can turn the sound on"
    );
    assert_eq!(
        NotifyDestination::parse_local("sound"),
        None,
        "sound is not a destination, so no level routes to it"
    );

    // Off is the default, not a removal: the switch still works when set.
    assert!(
        toml::from_str::<NotificationConfig>("sound = true")
            .expect("the switch parses")
            .sound,
        "the chime is opt-in, not unreachable"
    );
}
