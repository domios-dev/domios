//! What a tab whose provider registers no sessions actually produces at launch,
//! proved on the record — its level and its remedy field — never on its prose.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::json;

use super::*;
use crate::agent::provider::{
    AgentLaunchParams, AgentProvider, ReadinessStrategy, SystemPromptDelivery, TaskDelivery,
};
use crate::notification::bell::{self, BellPolicy};
use crate::notification::desktop::{self, DesktopPolicy};
use crate::notification::store::NotificationStore;
use crate::notification::NotifState;
use crate::service::notification::{
    create_notification_with_delivery, NotificationAdmission, PushDispatch, SurfaceDispatch,
};
use codeterm_types::config::schema::{AgentLevelChoice, NotificationConfig};
use codeterm_types::notification::NotifyDestination;

/// A provider that is neither claude nor codex, so nothing the mechanism does
/// can be keyed on a shipped provider id.
struct FixtureProvider {
    id: String,
    declares_hooks: bool,
    integration_status: serde_json::Value,
}

impl FixtureProvider {
    fn new(id: &str, declares_hooks: bool, integration_status: serde_json::Value) -> Self {
        Self {
            id: id.to_string(),
            declares_hooks,
            integration_status,
        }
    }
}

impl AgentProvider for FixtureProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn build_launch_command(&self, _params: &AgentLaunchParams) -> String {
        self.id.clone()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::default(),
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn system_prompt_delivery(&self) -> SystemPromptDelivery {
        SystemPromptDelivery::External
    }
    fn detect_from_title(&self, _title: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _text: &str) -> bool {
        false
    }
    fn session_file_path(&self, _cwd: &str, _session_id: &str) -> Option<PathBuf> {
        None
    }
    fn build_resume_command(&self, _session_id: &str, _skip: bool) -> Option<String> {
        None
    }
    fn detect_session_id(
        &self,
        _cwd: &str,
        _exclude: &[String],
        _max_age: Option<Duration>,
    ) -> Option<String> {
        None
    }
    fn has_hooks(&self) -> bool {
        self.declares_hooks
    }
    fn check_integration(&self) -> serde_json::Value {
        self.integration_status.clone()
    }
}

const FIXTURE_PROVIDER: &str = "fixture-agent";
const FIXTURE_TARGET: &str = "~/.fixture-agent/settings.json";

fn registry_with(provider: FixtureProvider) -> AgentRegistry {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(provider));
    registry
}

/// The shipped grid, then one level's whole row closed — the two ends of the
/// user's per-level destination configuration.
fn config_routing(level: NotifyLevel, routed: bool) -> NotificationConfig {
    NotificationConfig {
        agent_levels: NotifyLevel::ALL
            .into_iter()
            .map(|row_level| {
                let mut row = AgentLevelChoice::open(row_level.as_str());
                if row_level == level {
                    for destination in NotifyDestination::ALL {
                        row.set(destination, routed);
                    }
                }
                row
            })
            .collect(),
        ..NotificationConfig::default()
    }
}

struct Sinks {
    store: NotifState,
    native: Mutex<Vec<String>>,
    emitted: Mutex<Vec<serde_json::Value>>,
}

impl Sinks {
    fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(NotificationStore::new(10))),
            native: Mutex::new(Vec::new()),
            emitted: Mutex::new(Vec::new()),
        }
    }

    /// The announcement through the same code path `create_notification_ext`
    /// uses, with the surface policy read off a given config.
    fn announce(&self, silence: &IntegrationSilence, cfg: &NotificationConfig) -> String {
        let declared = silence.intent().level;
        create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &self.store,
            |title, _| self.native.lock().push(title.to_string()),
            silence.title(FIXTURE_PROVIDER),
            Some(silence.body()),
            Some("warning"),
            None,
            None,
            NotificationOrigin::claimed(Some("tab_launch".into())),
            |candidate| {
                Some(crate::notification::store::NotificationProvenance {
                    tab_id: Some(candidate.to_string()),
                    tab_title: Some("Fixture tab".into()),
                    workspace_id: None,
                    peer_name: None,
                })
            },
            PushDispatch::inert(),
            SurfaceDispatch {
                declared,
                bell: BellPolicy::from_config(cfg),
                desktop: DesktopPolicy::from_config(cfg),
            },
            |payload| self.emitted.lock().push(payload),
        )
    }

    fn records(&self) -> Vec<crate::notification::store::Notification> {
        self.store.lock().list(false).into_iter().cloned().collect()
    }
}

fn silence_of(verdict: LaunchVerdict) -> IntegrationSilence {
    match verdict {
        LaunchVerdict::Silent(silence) => silence,
        other => panic!("expected an announcement, got {other:?}"),
    }
}

/// The defect: a provider whose integration was never consented to registers no
/// sessions, and before this mechanism existed nothing said so.
#[test]
fn integration_silence_unconsented_provider_announces_at_launch() {
    let registry = registry_with(FixtureProvider::new(
        FIXTURE_PROVIDER,
        true,
        json!({ "session_start_hook": false, "hooks_installed": false, "target": FIXTURE_TARGET }),
    ));
    let (facts, display_name) = read_provider_facts(&registry, FIXTURE_PROVIDER, false)
        .expect("a registered provider reports its own facts");
    assert!(
        !facts.consent_recorded,
        "the fixture stands for a machine where consent was never given"
    );

    let silence = silence_of(evaluate(FIXTURE_PROVIDER, &facts));

    assert_eq!(
        silence.level,
        NotifyLevel::ActionNeeded,
        "the user must act before any session will register"
    );
    assert_eq!(
        silence.remedy,
        IntegrationRemedy::GrantConsentAndInstall {
            target: Some(FIXTURE_TARGET.to_string())
        },
        "the remedy names the consent-granting install path and the target the fixture supplied"
    );
    assert_eq!(silence.remedy.next_step(), IntegrationRemedy::INSTALL_PATH);

    let sinks = Sinks::new();
    let id = sinks.announce(&silence, &config_routing(silence.level, true));

    let records = sinks.records();
    assert_eq!(records.len(), 1, "the launch produced exactly one record");
    assert_eq!(records[0].id, id);
    assert_eq!(
        records[0].level,
        Some(NotifyLevel::ActionNeeded),
        "the stored record carries the declared level"
    );
    assert_eq!(
        records[0].tab_id.as_deref(),
        Some("tab_launch"),
        "the announcement is attributed to the tab that launched"
    );
    assert_eq!(
        sinks.native.lock().len(),
        1,
        "the user learns it without opening anything"
    );
    assert_eq!(display_name, FIXTURE_PROVIDER);
}

/// Negative control: an installed integration registers sessions, so there is
/// nothing to announce.
#[test]
fn integration_silence_installed_integration_announces_nothing() {
    let registry = registry_with(FixtureProvider::new(
        FIXTURE_PROVIDER,
        true,
        json!({ "session_start_hook": true, "hooks_installed": true, "target": FIXTURE_TARGET }),
    ));
    let (facts, _) = read_provider_facts(&registry, FIXTURE_PROVIDER, true).unwrap();

    assert_eq!(
        evaluate(FIXTURE_PROVIDER, &facts),
        LaunchVerdict::Registering,
        "an installed integration is not a silence"
    );

    let hookless = registry_with(FixtureProvider::new(
        "fixture-hookless",
        false,
        json!({ "status": "ok" }),
    ));
    let (hookless_facts, _) = read_provider_facts(&hookless, "fixture-hookless", false).unwrap();
    assert_eq!(
        evaluate("fixture-hookless", &hookless_facts),
        LaunchVerdict::Registering,
        "a provider that declares no hooks has no integration to miss"
    );

    let opaque = registry_with(FixtureProvider::new(
        "fixture-opaque",
        true,
        json!({ "status": "ok" }),
    ));
    let (opaque_facts, _) = read_provider_facts(&opaque, "fixture-opaque", false).unwrap();
    assert_eq!(
        evaluate("fixture-opaque", &opaque_facts),
        LaunchVerdict::Unreadable,
        "an unreadable install report says nothing rather than guessing"
    );
}

/// The announcement is subject to the user's per-level destination grid, not
/// delivered around it.
#[test]
fn integration_silence_announcement_routes_through_the_level_destination_matrix() {
    let registry = registry_with(FixtureProvider::new(
        FIXTURE_PROVIDER,
        true,
        json!({ "session_start_hook": false, "target": FIXTURE_TARGET }),
    ));
    let (facts, _) = read_provider_facts(&registry, FIXTURE_PROVIDER, false).unwrap();
    let silence = silence_of(evaluate(FIXTURE_PROVIDER, &facts));

    let open = Sinks::new();
    open.announce(&silence, &config_routing(silence.level, true));
    assert_eq!(open.records().len(), 1);
    assert_eq!(
        open.emitted.lock().len(),
        1,
        "with the level's row open the bell receives it"
    );
    assert_eq!(open.native.lock().len(), 1);

    let closed_config = config_routing(silence.level, false);
    let closed = Sinks::new();
    closed.announce(&silence, &closed_config);
    assert_eq!(
        closed.records().len(),
        1,
        "the record is kept: routing decides where it goes, not whether it exists"
    );
    assert!(
        closed.emitted.lock().is_empty(),
        "a level the user routed away from the bell never arrives there"
    );
    assert!(
        closed.native.lock().is_empty(),
        "nor at the system notification"
    );

    assert_eq!(
        bell::admit(
            Some(silence.level),
            &BellPolicy::from_config(&closed_config)
        ),
        bell::BellAdmission::LevelDisabled(silence.level),
        "the refusal names the level, so the gate is the shipped one"
    );
    assert_eq!(
        desktop::admit(
            Some(silence.level),
            &DesktopPolicy::from_config(&closed_config)
        ),
        desktop::DesktopAdmission::LevelDisabled(silence.level),
    );
}

/// A standing condition announces on transition. Ten launches under the same
/// condition are the same news.
#[test]
fn integration_silence_announces_on_transition_not_on_every_launch() {
    let registry = registry_with(FixtureProvider::new(
        FIXTURE_PROVIDER,
        true,
        json!({ "session_start_hook": false, "target": FIXTURE_TARGET }),
    ));
    let (facts, _) = read_provider_facts(&registry, FIXTURE_PROVIDER, false).unwrap();
    let silence = silence_of(evaluate(FIXTURE_PROVIDER, &facts));
    assert_eq!(
        silence.condition,
        crate::notification::dedupe::provider_integration_missing_condition(FIXTURE_PROVIDER),
        "the identity key is the provider's own standing condition"
    );

    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let observe = |present: bool| {
        let mut store = store.lock();
        if present {
            Some(store.observe_condition_present(MACHINE_SCOPE, &silence.condition))
        } else {
            store.observe_condition_absent(MACHINE_SCOPE, &silence.condition);
            None
        }
    };

    assert_eq!(observe(true), Some(ConditionTransition::Announce));
    for _ in 0..9 {
        assert_eq!(
            observe(true),
            Some(ConditionTransition::AlreadyAnnounced),
            "ten launches under one standing condition produce one announcement"
        );
    }

    observe(false);
    assert_eq!(
        observe(true),
        Some(ConditionTransition::Announce),
        "the integration going missing again is news again"
    );

    // The latch is machine-wide, so a second tab under the same condition is
    // still the same news.
    let other_condition = crate::notification::dedupe::provider_integration_missing_condition(
        "fixture-other-provider",
    );
    assert_eq!(
        store
            .lock()
            .observe_condition_present(MACHINE_SCOPE, &other_condition),
        ConditionTransition::Announce,
        "a different provider's gap is a different condition"
    );
}

/// The mechanism keys on the declared capability. A fixture provider that is
/// neither claude nor codex reaches the announcement on capability alone.
#[test]
fn integration_silence_keys_on_declared_capability_not_a_provider_id() {
    for provider_id in ["fixture-alpha", "fixture-beta"] {
        let registry = registry_with(FixtureProvider::new(
            provider_id,
            true,
            json!({ "hooks_installed": false, "target": FIXTURE_TARGET }),
        ));
        let (facts, display_name) = read_provider_facts(&registry, provider_id, false).unwrap();
        assert!(
            facts.registers_sessions_via_hooks,
            "{provider_id} declares the capability the check reads"
        );

        let silence = silence_of(evaluate(provider_id, &facts));
        assert_eq!(silence.provider_id, provider_id);
        assert_eq!(silence.level, NotifyLevel::ActionNeeded);
        assert_eq!(
            silence.remedy.target(),
            Some(FIXTURE_TARGET),
            "the remedy names the target the provider declared"
        );
        assert_eq!(display_name, provider_id);
    }

    // A consent record already stands, so the same capability names the other
    // arm of the same install path.
    let registry = registry_with(FixtureProvider::new(
        "fixture-gamma",
        true,
        json!({ "hooks_installed": false, "target": FIXTURE_TARGET }),
    ));
    let (facts, _) = read_provider_facts(&registry, "fixture-gamma", true).unwrap();
    assert_eq!(
        silence_of(evaluate("fixture-gamma", &facts)).remedy,
        IntegrationRemedy::ReinstallIntegration {
            target: Some(FIXTURE_TARGET.to_string())
        },
    );

    assert_eq!(
        read_provider_facts(&registry, "fixture-unregistered", false),
        None,
        "an unknown provider yields no facts rather than a guess"
    );
}
