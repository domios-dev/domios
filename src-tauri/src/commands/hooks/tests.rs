use super::{
    infer_consent_for_owned_install, install_and_record_integration,
    install_provider_integration_body, reconciliation_log_line, repair_integration_on_startup,
    resolve_precompact_choice, run_install_off_runtime, IntegrationRepairOutcome,
};
use crate::agent::provider::{
    install_integration_with_trust_grant, AgentLaunchParams, AgentProvider,
    IntegrationInstallOptions, IntegrationTrustGrantCapability, ReadinessStrategy, TaskDelivery,
};
use crate::agent::registry::AgentRegistry;
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::thread::ThreadId;
use std::time::Duration;

type ThreadProbe = Arc<StdMutex<Option<ThreadId>>>;

struct InstallProbeProvider {
    id: String,
    status: serde_json::Value,
    install_result: Result<(), String>,
    install_thread: ThreadProbe,
    precompact_choices: Arc<StdMutex<Vec<bool>>>,
    grant_outcome: Option<serde_json::Value>,
    grant_thread: ThreadProbe,
}

impl InstallProbeProvider {
    fn new(id: &str, install_result: Result<(), String>) -> Self {
        Self::with_status(id, install_result, serde_json::json!({}))
    }

    fn with_status(
        id: &str,
        install_result: Result<(), String>,
        status: serde_json::Value,
    ) -> Self {
        Self {
            id: id.to_string(),
            status,
            install_result,
            install_thread: Arc::new(StdMutex::new(None)),
            precompact_choices: Arc::new(StdMutex::new(Vec::new())),
            grant_outcome: None,
            grant_thread: Arc::new(StdMutex::new(None)),
        }
    }

    fn with_choice_sink(
        id: &str,
        install_result: Result<(), String>,
        status: serde_json::Value,
        precompact_choices: Arc<StdMutex<Vec<bool>>>,
    ) -> Self {
        Self {
            precompact_choices,
            ..Self::with_status(id, install_result, status)
        }
    }

    fn with_grant(id: &str, outcome: serde_json::Value) -> Self {
        Self {
            grant_outcome: Some(outcome),
            ..Self::new(id, Ok(()))
        }
    }
}

impl AgentProvider for InstallProbeProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn build_launch_command(&self, _params: &AgentLaunchParams) -> String {
        format!("{}-cli", self.id)
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::from_millis(0),
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
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
    fn build_resume_command(&self, _session_id: &str, _skip_permissions: bool) -> Option<String> {
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
    fn check_integration(&self) -> serde_json::Value {
        self.status.clone()
    }
    fn install_integration(
        &self,
        _api_port: u16,
        options: &IntegrationInstallOptions,
    ) -> Result<(), String> {
        *self.install_thread.lock().unwrap() = Some(std::thread::current().id());
        self.precompact_choices
            .lock()
            .unwrap()
            .push(options.precompact_memory_export);
        self.install_result.clone()
    }
    fn integration_trust_grant_capability(&self) -> Option<IntegrationTrustGrantCapability> {
        self.grant_outcome
            .as_ref()
            .map(|_| IntegrationTrustGrantCapability {
                managed_keys: vec!["session_start".into()],
            })
    }
    fn grant_integration_trust(&self, _managed_keys: &[String]) -> serde_json::Value {
        *self.grant_thread.lock().unwrap() = Some(std::thread::current().id());
        self.grant_outcome.clone().expect("grant outcome scripted")
    }
}

fn empty_config() -> crate::config::ConfigState {
    Arc::new(parking_lot::Mutex::new(
        crate::config::schema::AppConfig::default(),
    ))
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("current-thread runtime")
}

// The failing probe halts the body before audit persistence, so no test
// ever writes the real config file.
#[test]
fn install_offload_runs_provider_install_off_runtime_thread() {
    let probe = InstallProbeProvider::new("offload-probe", Err("halt before persist".into()));
    let install_thread = probe.install_thread.clone();
    let registry = Arc::new(AgentRegistry::new());
    registry.register_provider(Box::new(probe));
    let config = empty_config();

    let runtime_thread = std::thread::current().id();
    let result = current_thread_runtime().block_on(run_install_off_runtime(move || {
        install_provider_integration_body(
            &registry,
            &config,
            "offload-probe".into(),
            Some(7686),
            true,
            None,
        )
    }));

    let recorded = install_thread
        .lock()
        .unwrap()
        .expect("probe install must run");
    assert_ne!(recorded, runtime_thread);
    assert!(result.is_err());
}

#[test]
fn install_offload_error_path_round_trips_fixture_error() {
    let fixture_error = "probe install refused: fixture-supplied".to_string();
    let probe = InstallProbeProvider::new("offload-probe", Err(fixture_error.clone()));
    let registry = Arc::new(AgentRegistry::new());
    registry.register_provider(Box::new(probe));
    let config = empty_config();

    let result = current_thread_runtime().block_on(run_install_off_runtime(move || {
        install_provider_integration_body(
            &registry,
            &config,
            "offload-probe".into(),
            Some(7686),
            true,
            None,
        )
    }));

    assert_eq!(result, Err(fixture_error));
}

#[test]
fn refresh_without_precompact_flag_preserves_existing_precompact_hook() {
    let choices = Arc::new(StdMutex::new(Vec::new()));
    let registry = Arc::new(AgentRegistry::new());
    registry.register_provider(Box::new(InstallProbeProvider::with_choice_sink(
        "offload-probe",
        Err("halt before persist".into()),
        serde_json::json!({ "precompact_hook": true }),
        choices.clone(),
    )));
    let config = empty_config();
    config
        .lock()
        .agents
        .entry("offload-probe".into())
        .or_default()
        .extra
        .insert(
            "precompact_memory_export".into(),
            toml::Value::Boolean(false),
        );

    let result = install_provider_integration_body(
        &registry,
        &config,
        "offload-probe".into(),
        Some(7686),
        true,
        None,
    );

    assert!(result.is_err());
    assert_eq!(*choices.lock().unwrap(), vec![true]);
    let status = serde_json::json!({ "precompact_hook": true });
    assert!(resolve_precompact_choice(None, &status, Some(false)));
    assert!(resolve_precompact_choice(
        None,
        &serde_json::json!({}),
        Some(true)
    ));
    assert!(!resolve_precompact_choice(Some(false), &status, Some(true)));
    assert!(resolve_precompact_choice(
        Some(true),
        &serde_json::json!({}),
        Some(false)
    ));
}

#[test]
fn install_offload_success_path_round_trips_install_and_grant_result() {
    let outcome = serde_json::json!({ "granted": true, "marker": "fixture-supplied" });
    let probe = Arc::new(InstallProbeProvider::with_grant(
        "offload-probe",
        outcome.clone(),
    ));
    let install_thread = probe.install_thread.clone();
    let grant_thread = probe.grant_thread.clone();

    let runtime_thread = std::thread::current().id();
    let result = current_thread_runtime().block_on(run_install_off_runtime({
        let probe = probe.clone();
        move || {
            install_integration_with_trust_grant(
                probe.as_ref(),
                7686,
                &IntegrationInstallOptions {
                    consent: true,
                    precompact_memory_export: false,
                    repair: false,
                },
            )
        }
    }));

    assert_eq!(result, Ok(Some(outcome)));
    let installed_on = install_thread
        .lock()
        .unwrap()
        .expect("probe install must run");
    let granted_on = grant_thread.lock().unwrap().expect("probe grant must run");
    assert_ne!(installed_on, runtime_thread);
    assert_ne!(granted_on, runtime_thread);
}

#[test]
fn integration_transaction_rolls_back_when_audit_persistence_fails() {
    let installed = Cell::new(false);
    let rolled_back = Cell::new(false);
    let result = install_and_record_integration(
        || {
            installed.set(true);
            Ok(())
        },
        || Err("disk full".into()),
        || {
            rolled_back.set(true);
            Ok(())
        },
    );

    assert!(installed.get());
    assert!(rolled_back.get());
    assert!(result
        .unwrap_err()
        .contains("provider changes were restored"));
}

struct RepairProbeProvider {
    id: String,
    status: StdMutex<serde_json::Value>,
    writes: std::sync::atomic::AtomicUsize,
    consents: StdMutex<Vec<bool>>,
    precompact_choices: StdMutex<Vec<bool>>,
    repairs: StdMutex<Vec<bool>>,
    grant_calls: std::sync::atomic::AtomicUsize,
    error: Option<String>,
    live_session: StdMutex<String>,
}

impl RepairProbeProvider {
    fn new(status: serde_json::Value) -> Self {
        Self::with_id("fixture-provider", status)
    }

    fn with_id(id: &str, status: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            status: StdMutex::new(status),
            writes: std::sync::atomic::AtomicUsize::new(0),
            consents: StdMutex::new(Vec::new()),
            precompact_choices: StdMutex::new(Vec::new()),
            repairs: StdMutex::new(Vec::new()),
            grant_calls: std::sync::atomic::AtomicUsize::new(0),
            error: None,
            live_session: StdMutex::new("live-session-fixture".into()),
        }
    }

    fn unclassifiable(status: serde_json::Value) -> Self {
        Self {
            error: Some("foreign hook entry is unclassifiable".into()),
            ..Self::new(status)
        }
    }
}

fn reconcile_probe(
    provider: &RepairProbeProvider,
    previously_consented: bool,
    precompact_memory_export: bool,
) -> IntegrationRepairOutcome {
    let status = provider.check_integration();
    repair_integration_on_startup(
        provider,
        7686,
        &status,
        previously_consented,
        precompact_memory_export,
    )
}

impl AgentProvider for RepairProbeProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn build_launch_command(&self, _params: &AgentLaunchParams) -> String {
        "fixture".into()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
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
    fn build_resume_command(&self, _session_id: &str, _skip_permissions: bool) -> Option<String> {
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
    fn has_installable_integration(&self) -> bool {
        true
    }
    fn check_integration(&self) -> serde_json::Value {
        self.status.lock().unwrap().clone()
    }
    fn install_integration(
        &self,
        _api_port: u16,
        options: &IntegrationInstallOptions,
    ) -> Result<(), String> {
        self.consents.lock().unwrap().push(options.consent);
        self.precompact_choices
            .lock()
            .unwrap()
            .push(options.precompact_memory_export);
        self.repairs.lock().unwrap().push(options.repair);
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.status.lock().unwrap() = serde_json::json!({ "hooks_installed": true, "session_start_route": "tab-session-start" });
        Ok(())
    }
    fn integration_trust_grant_capability(&self) -> Option<IntegrationTrustGrantCapability> {
        Some(IntegrationTrustGrantCapability {
            managed_keys: vec!["session_start".into()],
        })
    }
    fn grant_integration_trust(&self, _managed_keys: &[String]) -> serde_json::Value {
        self.grant_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        serde_json::json!({ "granted": true })
    }
}

#[test]
fn hook_contract_repair_repairs_stale_contract_to_session_start_route() {
    let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
    reconcile_probe(&provider, true, false);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        provider.check_integration()["session_start_route"],
        "tab-session-start"
    );
}

#[test]
fn fresh_install_reconciles_after_recorded_consent() {
    let provider = RepairProbeProvider::new(serde_json::json!({
        "integration_state": "absent",
        "hooks_installed": false
    }));
    let outcome = reconcile_probe(&provider, true, false);
    assert_eq!(outcome, IntegrationRepairOutcome::Repaired);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(*provider.repairs.lock().unwrap(), vec![true]);
    assert_eq!(grants(&provider), 1);
}

#[test]
fn upgraded_owned_stale_hook_reconciles_after_recorded_consent() {
    let provider = RepairProbeProvider::new(serde_json::json!({
        "integration_state": "stale_version",
        "hooks_installed": true,
        "hooks_owned": true,
        "hook_contract_current": false,
        "session_start_hook": false,
        "stop_hook": false
    }));
    let outcome = reconcile_probe(&provider, true, false);
    assert_eq!(outcome, IntegrationRepairOutcome::Repaired);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(*provider.repairs.lock().unwrap(), vec![true]);
    assert_eq!(grants(&provider), 1);
    assert_eq!(
        provider.check_integration()["session_start_route"],
        "tab-session-start"
    );
}

#[test]
fn startup_reconciles_stale_claude_and_opencode_with_recorded_consent() {
    let claude = RepairProbeProvider::with_id(
        "claude",
        serde_json::json!({
            "hooks_installed": true,
            "integration_owned": true,
            "hook_contract_current": false,
            "precompact_hook": true
        }),
    );
    let opencode = RepairProbeProvider::with_id(
        "opencode",
        serde_json::json!({
            "plugin_installed": true,
            "plugin_file_owned": true,
            "plugin_file_current": false
        }),
    );

    assert_eq!(
        reconcile_probe(&claude, true, true),
        IntegrationRepairOutcome::Repaired
    );
    assert_eq!(
        reconcile_probe(&opencode, true, false),
        IntegrationRepairOutcome::Repaired
    );
    assert_eq!(claude.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(opencode.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(*claude.precompact_choices.lock().unwrap(), vec![true]);
}

#[test]
fn owned_install_without_consent_record_is_treated_as_consented_once() {
    let status = serde_json::json!({
        "hooks_installed": true,
        "integration_owned": true,
        "hook_contract_current": false,
        "precompact_hook": true
    });
    let mut config = crate::config::schema::AppConfig::default();

    assert!(infer_consent_for_owned_install(
        &mut config,
        "claude",
        &status,
        1_790_403_600,
        true,
    ));
    let provider_config = &config.agents["claude"];
    assert_eq!(
        provider_config.extra[crate::config::schema::INTEGRATION_CONSENT_GRANTED].as_bool(),
        Some(true)
    );
    assert_eq!(
        provider_config.extra["integration_consent_recorded_at"].as_integer(),
        Some(1_790_403_600)
    );
    assert_eq!(
        provider_config.extra["integration_consent_note"].as_str(),
        Some("inferred from an existing CodeTerm-owned integration")
    );
    assert_eq!(
        provider_config.extra["precompact_memory_export"].as_bool(),
        Some(true)
    );
    assert!(!infer_consent_for_owned_install(
        &mut config,
        "claude",
        &status,
        1_790_403_700,
        false,
    ));
    assert_eq!(
        config.agents["claude"].extra["integration_consent_recorded_at"].as_integer(),
        Some(1_790_403_600)
    );
    let legacy_status = serde_json::json!({
        "hooks_installed": true,
        "hooks_owned": true,
        "integration_owned": false,
        "hook_contract_current": false
    });
    assert!(!infer_consent_for_owned_install(
        &mut config,
        "legacy-provider",
        &legacy_status,
        1_790_403_700,
        false,
    ));

    let provider = RepairProbeProvider::with_id("claude", status);
    assert_eq!(
        reconcile_probe(&provider, true, false),
        IntegrationRepairOutcome::Repaired
    );
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn unowned_hooks_are_never_rewritten_at_startup() {
    let provider = RepairProbeProvider::new(serde_json::json!({
        "hooks_installed": true,
        "hooks_owned": false,
        "hook_contract_current": false
    }));
    let unowned_plugin = RepairProbeProvider::with_id(
        "opencode",
        serde_json::json!({
            "plugin_installed": true,
            "plugin_file_owned": false,
            "plugin_file_current": false
        }),
    );

    assert_eq!(
        reconcile_probe(&provider, false, false),
        IntegrationRepairOutcome::ConsentMissing
    );
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(provider.consents.lock().unwrap().is_empty());
    assert_eq!(
        reconcile_probe(&unowned_plugin, false, false),
        IntegrationRepairOutcome::ConsentMissing
    );
    assert_eq!(
        unowned_plugin
            .writes
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[test]
fn reconciliation_outcomes_have_one_summary_line_and_inference_note() {
    let cases = [
        (IntegrationRepairOutcome::Current, "current"),
        (IntegrationRepairOutcome::Repaired, "repaired"),
        (IntegrationRepairOutcome::ConsentMissing, "consent missing"),
        (
            IntegrationRepairOutcome::TrustGrantRequired,
            "trust required",
        ),
        (IntegrationRepairOutcome::Unreadable, "unreadable"),
        (
            IntegrationRepairOutcome::Skipped("fixture reason".into()),
            "skipped (fixture reason)",
        ),
    ];
    for (outcome, expected) in cases {
        let line = reconciliation_log_line(&outcome, true);
        assert!(line.starts_with("integration reconciliation: "));
        assert!(line.contains(expected));
        assert!(line.contains("consent inferred from existing CodeTerm-owned install"));
        assert!(!line.contains('\n'));
    }
}

#[test]
fn hook_contract_repair_is_idempotent_by_write_count() {
    let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
    reconcile_probe(&provider, true, false);
    reconcile_probe(&provider, true, false);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
}

fn grants(provider: &RepairProbeProvider) -> usize {
    provider
        .grant_calls
        .load(std::sync::atomic::Ordering::SeqCst)
}

#[test]
fn hook_contract_repair_uses_recorded_consent_and_grants_trust_in_the_same_step() {
    let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
    reconcile_probe(&provider, false, false);
    assert!(provider.consents.lock().unwrap().is_empty());
    assert_eq!(grants(&provider), 0);
    reconcile_probe(&provider, true, false);
    assert_eq!(*provider.consents.lock().unwrap(), vec![true]);
    assert_eq!(*provider.repairs.lock().unwrap(), vec![true]);
    assert_eq!(grants(&provider), 1);
}

#[test]
fn untrusted_hash_with_recorded_consent_is_rewritten_and_regranted() {
    let provider =
        RepairProbeProvider::new(serde_json::json!({ "integration_state": "untrusted_hash" }));
    let outcome = reconcile_probe(&provider, true, false);
    assert_eq!(outcome, IntegrationRepairOutcome::Repaired);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(grants(&provider), 1);
}

#[test]
fn untrusted_hash_without_consent_is_left_untouched() {
    let provider =
        RepairProbeProvider::new(serde_json::json!({ "integration_state": "untrusted_hash" }));
    let outcome = reconcile_probe(&provider, false, false);
    assert_eq!(outcome, IntegrationRepairOutcome::TrustGrantRequired);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(provider.consents.lock().unwrap().is_empty());
    assert_eq!(grants(&provider), 0);
}

#[test]
fn failed_repair_grants_no_trust() {
    let provider =
        RepairProbeProvider::unclassifiable(serde_json::json!({ "hooks_installed": false }));
    reconcile_probe(&provider, true, false);
    assert_eq!(grants(&provider), 0);
}

#[test]
fn hook_contract_repair_preserves_live_session() {
    let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
    let before = provider.live_session.lock().unwrap().clone();
    reconcile_probe(&provider, true, false);
    assert_eq!(*provider.live_session.lock().unwrap(), before);
}

#[test]
fn hook_contract_repair_is_capability_driven_for_non_claude_fixture() {
    let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
    assert_ne!(provider.id(), "claude");
    assert!(provider.has_installable_integration());
    reconcile_probe(&provider, true, false);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn hook_contract_repair_leaves_unclassifiable_entry_and_reports_outcome() {
    let provider =
        RepairProbeProvider::unclassifiable(serde_json::json!({ "hooks_installed": false }));
    let outcome = reconcile_probe(&provider, true, false);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(*provider.consents.lock().unwrap(), vec![true]);
    assert!(matches!(outcome, IntegrationRepairOutcome::RepairFailed(_)));
}
