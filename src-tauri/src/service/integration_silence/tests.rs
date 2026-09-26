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
    let registered: Vec<&'static str> = crate::notification::channels::roster()
        .into_iter()
        .map(|entry| entry.id())
        .collect();
    NotificationConfig {
        desktop: true,
        agent_levels: NotifyLevel::ALL
            .into_iter()
            .map(|row_level| {
                let mut row = AgentLevelChoice::open(row_level.as_str(), &registered);
                if row_level == level {
                    for destination in NotifyDestination::columns(registered.clone()) {
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
            crate::notification::store::Ask::default(),
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

#[test]
fn integration_silence_states_have_distinct_constants_and_remedies() {
    let cases = [
        (
            INTEGRATION_STATE_ABSENT,
            IntegrationState::Absent,
            IntegrationRemedy::INSTALL_PATH,
            false,
        ),
        (
            INTEGRATION_STATE_STALE_VERSION,
            IntegrationState::StaleVersion,
            IntegrationRemedy::REFRESH_PATH,
            true,
        ),
        (
            INTEGRATION_STATE_UNTRUSTED_HASH,
            IntegrationState::UntrustedHash,
            IntegrationRemedy::TRUST_PATH,
            true,
        ),
    ];
    for (state_constant, state, remedy_path, consent_recorded) in cases {
        let registry = registry_with(FixtureProvider::new(
            FIXTURE_PROVIDER,
            true,
            json!({
                "integration_state": state_constant,
                "session_start_hook": false,
                "hooks_installed": false,
                "target": FIXTURE_TARGET
            }),
        ));
        let (facts, _) =
            read_provider_facts(&registry, FIXTURE_PROVIDER, consent_recorded).unwrap();
        assert_eq!(facts.state, Some(state));
        assert_eq!(state.as_str(), state_constant);
        let silence = silence_of(evaluate(FIXTURE_PROVIDER, &facts));
        assert_eq!(silence.state, state);
        assert_eq!(silence.remedy.next_step(), remedy_path);
    }
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
/// delivered around it — and the panel, which is the complete log, keeps it
/// either way.
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
        "with the level's row open the panel receives it"
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
    assert_eq!(
        closed.emitted.lock().len(),
        1,
        "and it still reaches the panel, which is the log rather than a destination"
    );
    assert!(
        closed.native.lock().is_empty(),
        "while the system notification, which can refuse, does"
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

fn facts_with_hash(state: IntegrationState, hash: Option<&str>) -> IntegrationFacts {
    IntegrationFacts {
        registers_sessions_via_hooks: true,
        installed: Some(false),
        consent_recorded: true,
        integration_owned: false,
        target: Some(FIXTURE_TARGET.to_string()),
        state: Some(state),
        content_hash: hash.map(str::to_string),
        contract_current: None,
    }
}

/// The provider's own content hash is what identifies the state, so two
/// instances that wrote byte-identical hooks report one state, not two.
#[test]
fn identical_content_hashes_are_one_observed_state() {
    let from_dev = facts_with_hash(IntegrationState::StaleVersion, Some("7f3a2b10"));
    let from_prod = facts_with_hash(IntegrationState::StaleVersion, Some("7f3a2b10"));
    let after_a_real_change = facts_with_hash(IntegrationState::StaleVersion, Some("00c0ffee"));

    assert_eq!(
        AnnouncedIntegrationState::observed(IntegrationState::StaleVersion, &from_dev),
        AnnouncedIntegrationState::observed(IntegrationState::StaleVersion, &from_prod),
        "byte-identical content is one state whichever instance observed it"
    );
    assert_ne!(
        AnnouncedIntegrationState::observed(IntegrationState::StaleVersion, &from_dev),
        AnnouncedIntegrationState::observed(IntegrationState::StaleVersion, &after_a_real_change),
        "only content that actually changed is a different state"
    );
}

/// The in-memory condition ledger is rebuilt empty on every daemon start, so
/// the record that keeps an unchanged gap quiet has to outlive the process.
#[test]
fn an_unchanged_state_does_not_re_announce_on_a_later_daemon_start() {
    let provider = "fixture-announce-latch-provider";
    clear_announced(provider);
    let facts = facts_with_hash(IntegrationState::StaleVersion, Some("7f3a2b10"));
    let observed = AnnouncedIntegrationState::observed(IntegrationState::StaleVersion, &facts);

    assert!(
        is_news(provider, &observed),
        "the first observation is news"
    );
    record_announced(provider, &observed);
    assert!(
        !is_news(provider, &observed),
        "a later daemon start observing the same state must not re-announce"
    );

    let changed = AnnouncedIntegrationState::observed(
        IntegrationState::StaleVersion,
        &facts_with_hash(IntegrationState::StaleVersion, Some("00c0ffee")),
    );
    assert!(is_news(provider, &changed), "changed content is news again");

    let other_state = AnnouncedIntegrationState::observed(IntegrationState::UntrustedHash, &facts);
    assert!(
        is_news(provider, &other_state),
        "a different integration state is news even at the same content"
    );

    clear_announced(provider);
    assert!(
        is_news(provider, &observed),
        "the gap closing re-arms the next announcement"
    );
}

/// A provider that publishes no hash still gets a state-only latch, so the
/// mechanism never invents an identity it did not observe.
#[test]
fn a_provider_without_a_content_hash_latches_on_state_alone() {
    let provider = "fixture-announce-latch-no-hash";
    clear_announced(provider);
    let facts = facts_with_hash(IntegrationState::Absent, None);
    let observed = AnnouncedIntegrationState::observed(IntegrationState::Absent, &facts);

    assert_eq!(observed.content_hash, None);
    assert!(is_news(provider, &observed));
    record_announced(provider, &observed);
    assert!(!is_news(provider, &observed));
    clear_announced(provider);
}

#[test]
fn content_hash_is_read_from_the_providers_own_report() {
    assert_eq!(
        read_content_hash(&json!({ "hook_content_hash": "7f3a2b10" })).as_deref(),
        Some("7f3a2b10")
    );
    assert_eq!(
        read_content_hash(&json!({ "hook_content_hash": "  " })),
        None
    );
    assert_eq!(read_content_hash(&json!({})), None);
}

fn concurrent_observed(id: &str) -> AnnouncedIntegrationState {
    AnnouncedIntegrationState {
        state: INTEGRATION_STATE_STALE_VERSION.to_string(),
        content_hash: Some(format!("hash-{id}")),
    }
}

/// The defect this replaced: an unguarded read-modify-write across one shared
/// file. Two writers truncated each other and left JSON that parsed as
/// "nothing was ever announced", so a standing gap re-announced every restart.
#[test]
fn concurrent_records_neither_lose_a_key_nor_tear_the_file() {
    let ids: Vec<String> = (0..8)
        .map(|i| format!("fixture-announce-concurrent-{i}"))
        .collect();
    for id in &ids {
        clear_announced(id);
    }

    std::thread::scope(|scope| {
        for id in &ids {
            scope.spawn(move || record_announced(id, &concurrent_observed(id)));
        }
    });

    for id in &ids {
        assert!(
            !is_news(id, &concurrent_observed(id)),
            "{id} lost its record to a concurrent writer"
        );
    }

    // The durable copy has to survive the same race, or the latch is rebuilt
    // empty on the next start and every gap announces again.
    let raw = std::fs::read_to_string(announced_path()).expect("state file written");
    let parsed: std::collections::HashMap<String, AnnouncedIntegrationState> =
        serde_json::from_str(&raw).expect("a concurrent write must never leave unparsable JSON");
    for id in &ids {
        assert_eq!(
            parsed.get(id),
            Some(&concurrent_observed(id)),
            "{id} is missing from the persisted map"
        );
        clear_announced(id);
    }
}

/// The daemon-start gate. Every branch is read off the provider's own report
/// plus the consent record, never off which provider it is.
#[test]
fn reconciliation_is_authorized_only_by_consent_and_the_install_report() {
    fn facts(status: serde_json::Value, consent_recorded: bool) -> IntegrationFacts {
        facts_from_status(&status, true, consent_recorded)
    }

    // A working but pre-marker install: repaired silently under recorded
    // consent, and left alone without it.
    let unmarked = json!({ "hooks_installed": true, "hook_contract_current": false });
    assert_eq!(
        authorize_reconciliation(&facts(unmarked.clone(), true)),
        ReconcileAuthorization::Reconcile
    );
    assert_eq!(
        authorize_reconciliation(&facts(unmarked, false)),
        ReconcileAuthorization::ConsentMissing
    );

    // Already carrying the current contract: nothing to write, so a second
    // daemon observing the same state must not touch the shared file.
    assert_eq!(
        authorize_reconciliation(&facts(
            json!({ "hooks_installed": true, "hook_contract_current": true }),
            true
        )),
        ReconcileAuthorization::AlreadyCurrent
    );
    assert_eq!(
        authorize_reconciliation(&facts(
            json!({ "hooks_installed": true, "hook_contract_current": true }),
            false
        )),
        ReconcileAuthorization::ConsentMissing
    );
    // A provider that publishes no contract field is current when installed.
    assert_eq!(
        authorize_reconciliation(&facts(json!({ "hooks_installed": true }), true)),
        ReconcileAuthorization::AlreadyCurrent
    );

    // Missing and stale both reconcile once consent is on record.
    for status in [
        json!({ "hooks_installed": false }),
        json!({ "integration_state": INTEGRATION_STATE_STALE_VERSION }),
    ] {
        assert_eq!(
            authorize_reconciliation(&facts(status.clone(), true)),
            ReconcileAuthorization::Reconcile
        );
        assert_eq!(
            authorize_reconciliation(&facts(status, false)),
            ReconcileAuthorization::ConsentMissing
        );
    }

    // Trust is granted by content hash. Recorded consent covers the rewrite
    // plus the grant; without it the untrusted hooks are left alone.
    let untrusted = json!({ "integration_state": INTEGRATION_STATE_UNTRUSTED_HASH });
    assert_eq!(
        authorize_reconciliation(&facts(untrusted.clone(), true)),
        ReconcileAuthorization::Reconcile
    );
    assert_eq!(
        authorize_reconciliation(&facts(untrusted, false)),
        ReconcileAuthorization::TrustGrantRequired
    );

    // An unreadable report stays unreadable; it never becomes a repair.
    assert_eq!(
        authorize_reconciliation(&facts(json!({ "something_else": 1 }), true)),
        ReconcileAuthorization::Unreadable
    );
}

#[test]
fn integration_ownership_is_read_from_provider_report_fields() {
    assert!(read_integration_owned(
        &json!({ "integration_owned": true })
    ));
    assert!(read_integration_owned(&json!({ "hooks_owned": true })));
    assert!(read_integration_owned(
        &json!({ "plugin_file_owned": true })
    ));
    assert!(!read_integration_owned(&json!({ "hooks_installed": true })));
}
