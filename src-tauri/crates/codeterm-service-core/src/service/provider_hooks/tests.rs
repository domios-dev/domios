use super::{
    infer_consent_for_owned_install, install_and_record_integration,
    install_provider_integration_body, reconciliation_log_line, repair_integration_on_startup,
    restore_integration_for_provider, IntegrationRepairOutcome,
};
use crate::agent::provider::{
    install_integration_with_trust_grant, AgentLaunchParams, AgentProvider,
    IntegrationInstallOptions, IntegrationKind, IntegrationTrustGrantCapability, ReadinessStrategy,
    TaskDelivery,
};
use crate::agent::registry::AgentRegistry;
use codeterm_plugin::agent_claude::provider::hook_ownership::is_codeterm_hook;
use codeterm_plugin::agent_claude::provider::integration::install_integration_at;
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::thread::ThreadId;
use std::time::Duration;

type ThreadProbe = Arc<StdMutex<Option<ThreadId>>>;

const LEGACY_PANE_SESSION_START: &str = r#"curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/session_start -H 'Content-Type: application/json' -H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" -d '{}' > /dev/null 2>&1 || true"#;
const FOREIGN_PANE_ROUTE: &str =
    "curl http://localhost:7686/api/panes/$CODETERM_PANE_ID/hook/session_start";

struct InstallProbeProvider {
    id: String,
    integration_kind: IntegrationKind,
    status: serde_json::Value,
    install_result: Result<(), String>,
    install_thread: ThreadProbe,

    grant_outcome: Option<serde_json::Value>,
    grant_thread: ThreadProbe,
    integration_calls: Arc<StdMutex<Vec<&'static str>>>,
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
            integration_kind: IntegrationKind::None,
            status,
            install_result,
            install_thread: Arc::new(StdMutex::new(None)),

            grant_outcome: None,
            grant_thread: Arc::new(StdMutex::new(None)),
            integration_calls: Arc::new(StdMutex::new(Vec::new())),
        }
    }

    fn with_kind(
        id: &str,
        integration_kind: IntegrationKind,
        install_result: Result<(), String>,
        status: serde_json::Value,
    ) -> Self {
        Self {
            integration_kind,
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
        self.integration_calls.lock().unwrap().push("status");
        self.status.clone()
    }
    fn integration_kind(&self) -> IntegrationKind {
        self.integration_calls.lock().unwrap().push("kind");
        self.integration_kind
    }
    fn ensure_integration_on_startup(&self, _api_port: u16) {
        self.integration_calls.lock().unwrap().push("startup");
    }
    fn install_integration(
        &self,
        _api_port: u16,
        _options: &IntegrationInstallOptions,
    ) -> Result<(), String> {
        self.integration_calls.lock().unwrap().push("install");
        *self.install_thread.lock().unwrap() = Some(std::thread::current().id());
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

#[test]
fn startup_reconciliation_machine_owner_false_never_calls_provider_or_changes_consent() {
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set("CODETERM_MACHINE_OWNER", "false");
    env.set("CODETERM_INSTANCE", "nonowner-integration-test");
    let provider = Arc::new(InstallProbeProvider::new("nonowner", Ok(())));
    let config = empty_config();
    config
        .lock()
        .agents
        .entry("nonowner".into())
        .or_default()
        .extra
        .insert(
            crate::config::schema::INTEGRATION_CONSENT_GRANTED.into(),
            toml::Value::Boolean(true),
        );
    let before = serde_json::to_value(&*config.lock()).unwrap();
    let consented = config.lock().agents.clone();

    super::spawn_reconciliation(provider.clone(), consented, Some(config.clone()), None);

    assert!(provider.integration_calls.lock().unwrap().is_empty());
    assert!(provider.install_thread.lock().unwrap().is_none());
    assert!(provider.grant_thread.lock().unwrap().is_none());
    assert_eq!(serde_json::to_value(&*config.lock()).unwrap(), before);
}

#[test]
fn startup_reconciliation_machine_owner_true_keeps_provider_capability_check() {
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set("CODETERM_MACHINE_OWNER", "true");
    let provider = Arc::new(InstallProbeProvider::new("owner-no-integration", Ok(())));

    super::spawn_reconciliation(provider.clone(), Default::default(), None, None);

    assert_eq!(*provider.integration_calls.lock().unwrap(), vec!["kind"]);
}

#[test]
fn install_error_path_round_trips_fixture_error() {
    let fixture_error = "probe install refused: fixture-supplied".to_string();
    let probe = InstallProbeProvider::new("offload-probe", Err(fixture_error.clone()));
    let registry = Arc::new(AgentRegistry::new());
    registry.register_provider(Box::new(probe));
    let config = empty_config();

    let result = install_provider_integration_body(
        &registry,
        &config,
        "offload-probe".into(),
        Some(7686),
        true,
    );

    assert_eq!(result, Err(fixture_error));
}

#[test]
fn install_success_path_round_trips_install_and_grant_result() {
    let outcome = serde_json::json!({ "granted": true, "marker": "fixture-supplied" });
    let probe = InstallProbeProvider::with_grant("grant-probe", outcome.clone());
    let result = install_integration_with_trust_grant(
        &probe,
        7686,
        &IntegrationInstallOptions {
            consent: true,
            repair: false,
        },
    );
    assert_eq!(result, Ok(Some(outcome)));
    assert!(probe.install_thread.lock().unwrap().is_some());
    assert!(probe.grant_thread.lock().unwrap().is_some());
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
    integration_kind: IntegrationKind,
    status: StdMutex<serde_json::Value>,
    writes: std::sync::atomic::AtomicUsize,
    consents: StdMutex<Vec<bool>>,

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
        Self::with_kind(id, IntegrationKind::ProviderManaged, status)
    }

    fn with_kind(id: &str, integration_kind: IntegrationKind, status: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            integration_kind,
            status: StdMutex::new(status),
            writes: std::sync::atomic::AtomicUsize::new(0),
            consents: StdMutex::new(Vec::new()),

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
) -> IntegrationRepairOutcome {
    let status = provider.check_integration();
    repair_integration_on_startup(provider, 7686, &status, previously_consented)
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
    fn integration_kind(&self) -> IntegrationKind {
        self.integration_kind
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
        self.repairs.lock().unwrap().push(options.repair);
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.status.lock().unwrap() = match self.integration_kind {
            IntegrationKind::PluginFile => serde_json::json!({
                "plugin_installed": true,
                "plugin_file_exists": true,
                "plugin_file_owned": true,
                "plugin_file_current": true
            }),
            _ => serde_json::json!({
                "hooks_installed": true,
                "hook_contract_current": true,
                "session_start_route": "tab-session-start"
            }),
        };
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
    reconcile_probe(&provider, true);
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
    let outcome = reconcile_probe(&provider, true);
    assert_eq!(outcome, repaired_with_grant());
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(*provider.repairs.lock().unwrap(), vec![true]);
    assert_eq!(grants(&provider), 1);
    assert_eq!(provider.check_integration()["hook_contract_current"], true);
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
    let outcome = reconcile_probe(&provider, true);
    assert_eq!(outcome, repaired_with_grant());
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
            "hook_contract_current": false
        }),
    );
    let opencode = RepairProbeProvider::with_kind(
        "opencode",
        IntegrationKind::PluginFile,
        serde_json::json!({
            "plugin_installed": true,
            "plugin_file_exists": true,
            "plugin_file_owned": true,
            "plugin_file_current": false
        }),
    );

    assert_eq!(reconcile_probe(&claude, true), repaired_with_grant());
    assert_eq!(reconcile_probe(&opencode, true), repaired_with_grant());
    assert_eq!(claude.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(opencode.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn owned_install_without_consent_record_is_treated_as_consented_once() {
    let status = serde_json::json!({
        "hooks_installed": true,
        "integration_owned": true,
        "hook_contract_current": false
    });
    let mut config = crate::config::schema::AppConfig::default();

    assert!(infer_consent_for_owned_install(
        &mut config,
        "claude",
        &status,
        1_790_403_600,
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
    assert!(!infer_consent_for_owned_install(
        &mut config,
        "claude",
        &status,
        1_790_403_700,
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
    ));

    let provider = RepairProbeProvider::with_id("claude", status);
    assert_eq!(reconcile_probe(&provider, true), repaired_with_grant());
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn opencode_plugin_file_integration_is_reconciled_at_startup() {
    let status = serde_json::json!({
        "plugin_installed": true,
        "plugin_file_exists": true,
        "plugin_file_owned": true,
        "plugin_file_current": false
    });
    let mut config = crate::config::schema::AppConfig::default();

    assert!(infer_consent_for_owned_install(
        &mut config,
        "opencode",
        &status,
        1_790_403_600,
    ));
    assert!(!infer_consent_for_owned_install(
        &mut config,
        "opencode",
        &status,
        1_790_403_700,
    ));
    assert_eq!(
        config.agents["opencode"].extra["integration_consent_recorded_at"].as_integer(),
        Some(1_790_403_600)
    );

    let provider = RepairProbeProvider::with_kind("opencode", IntegrationKind::PluginFile, status);
    assert!(provider.has_installable_integration());
    let outcome = reconcile_probe(&provider, true);
    assert_eq!(outcome, repaired_with_grant());
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        provider.check_integration()["plugin_file_current"].as_bool(),
        Some(true)
    );
}

#[test]
fn unmarked_opencode_plugin_file_is_never_rewritten() {
    let status = serde_json::json!({
        "plugin_installed": true,
        "plugin_file_exists": true,
        "plugin_file_owned": false,
        "plugin_file_current": false
    });
    let provider =
        RepairProbeProvider::with_kind("opencode", IntegrationKind::PluginFile, status.clone());
    let mut config = crate::config::schema::AppConfig::default();

    assert!(!infer_consent_for_owned_install(
        &mut config,
        "opencode",
        &status,
        1_790_403_600,
    ));
    assert_eq!(
        reconcile_probe(&provider, false),
        IntegrationRepairOutcome::ConsentMissing
    );
    assert_eq!(
        reconcile_probe(&provider, true),
        IntegrationRepairOutcome::ForeignPluginFilePreserved
    );
    assert_eq!(
        restore_integration_for_provider(&provider),
        Err("plugin-file integrations have no backup to restore".into())
    );
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(provider.check_integration(), status);
    assert_eq!(
        reconciliation_log_line(&IntegrationRepairOutcome::ConsentMissing, false),
        "integration reconciliation: consent missing"
    );

    let install_probe = InstallProbeProvider::with_kind(
        "opencode-install-probe",
        IntegrationKind::PluginFile,
        Ok(()),
        status,
    );
    let registry = Arc::new(AgentRegistry::new());
    let install_thread = install_probe.install_thread.clone();
    registry.register_provider(Box::new(install_probe));
    let result = install_provider_integration_body(
        &registry,
        &empty_config(),
        "opencode-install-probe".into(),
        Some(7686),
        true,
    );
    assert!(result.is_err());
    assert!(install_thread.lock().unwrap().is_none());
}

#[test]
fn explicit_consent_false_blocks_owned_install_inference() {
    let status = serde_json::json!({
        "hooks_installed": true,
        "integration_owned": true,
        "hook_contract_current": false
    });
    let mut config = crate::config::schema::AppConfig::default();
    config
        .agents
        .entry("claude".into())
        .or_default()
        .extra
        .insert(
            crate::config::schema::INTEGRATION_CONSENT_GRANTED.into(),
            toml::Value::Boolean(false),
        );

    assert!(!infer_consent_for_owned_install(
        &mut config,
        "claude",
        &status,
        1_790_403_600,
    ));
    let provider_config = &config.agents["claude"];
    assert_eq!(
        provider_config.extra[crate::config::schema::INTEGRATION_CONSENT_GRANTED].as_bool(),
        Some(false)
    );
    assert!(!provider_config
        .extra
        .contains_key("integration_consent_recorded_at"));
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
        reconcile_probe(&provider, false),
        IntegrationRepairOutcome::ConsentMissing
    );
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(provider.consents.lock().unwrap().is_empty());
    assert_eq!(
        reconcile_probe(&unowned_plugin, false),
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
fn reconciliation_returns_structured_repair_outcome_with_trust_grant() {
    let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
    assert_eq!(reconcile_probe(&provider, true), repaired_with_grant());
}

#[test]
fn hook_contract_repair_is_idempotent_by_write_count() {
    let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
    reconcile_probe(&provider, true);
    reconcile_probe(&provider, true);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
}

fn grants(provider: &RepairProbeProvider) -> usize {
    provider
        .grant_calls
        .load(std::sync::atomic::Ordering::SeqCst)
}

fn repaired_with_grant() -> IntegrationRepairOutcome {
    IntegrationRepairOutcome::Repaired(Some(serde_json::json!({ "granted": true })))
}

#[test]
fn hook_contract_repair_uses_recorded_consent_and_grants_trust_in_the_same_step() {
    let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
    reconcile_probe(&provider, false);
    assert!(provider.consents.lock().unwrap().is_empty());
    assert_eq!(grants(&provider), 0);
    reconcile_probe(&provider, true);
    assert_eq!(*provider.consents.lock().unwrap(), vec![true]);
    assert_eq!(*provider.repairs.lock().unwrap(), vec![true]);
    assert_eq!(grants(&provider), 1);
}

#[test]
fn untrusted_hash_with_recorded_consent_is_rewritten_and_regranted() {
    let provider =
        RepairProbeProvider::new(serde_json::json!({ "integration_state": "untrusted_hash" }));
    let outcome = reconcile_probe(&provider, true);
    assert_eq!(outcome, repaired_with_grant());
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(grants(&provider), 1);
}

#[test]
fn untrusted_hash_without_consent_is_left_untouched() {
    let provider =
        RepairProbeProvider::new(serde_json::json!({ "integration_state": "untrusted_hash" }));
    let outcome = reconcile_probe(&provider, false);
    assert_eq!(outcome, IntegrationRepairOutcome::TrustGrantRequired);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(provider.consents.lock().unwrap().is_empty());
    assert_eq!(grants(&provider), 0);
}

#[test]
fn failed_repair_grants_no_trust() {
    let provider =
        RepairProbeProvider::unclassifiable(serde_json::json!({ "hooks_installed": false }));
    reconcile_probe(&provider, true);
    assert_eq!(grants(&provider), 0);
}

#[test]
fn hook_contract_repair_preserves_live_session() {
    let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
    let before = provider.live_session.lock().unwrap().clone();
    reconcile_probe(&provider, true);
    assert_eq!(*provider.live_session.lock().unwrap(), before);
}

#[test]
fn hook_contract_repair_is_capability_driven_for_non_claude_fixture() {
    let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
    assert_ne!(provider.id(), "claude");
    assert!(provider.has_installable_integration());
    reconcile_probe(&provider, true);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn hook_contract_repair_leaves_unclassifiable_entry_and_reports_outcome() {
    let provider =
        RepairProbeProvider::unclassifiable(serde_json::json!({ "hooks_installed": false }));
    let outcome = reconcile_probe(&provider, true);
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(*provider.consents.lock().unwrap(), vec![true]);
    assert!(matches!(outcome, IntegrationRepairOutcome::RepairFailed(_)));
}

#[test]
fn hooks_legacy_pane_fingerprint_is_owned_and_repaired() {
    let legacy_entry = serde_json::json!({
        "hooks": [{ "type": "command", "command": LEGACY_PANE_SESSION_START }]
    });
    let status = serde_json::json!({
        "hooks_installed": false,
        "integration_owned": is_codeterm_hook(&legacy_entry),
        "hook_contract_current": false
    });
    assert_eq!(status["integration_owned"], true);

    let mut config = crate::config::schema::AppConfig::default();
    assert!(infer_consent_for_owned_install(
        &mut config,
        "claude",
        &status,
        1_790_403_600,
    ));
    let provider = RepairProbeProvider::with_id("claude", status);
    assert_eq!(reconcile_probe(&provider, true), repaired_with_grant());
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(*provider.repairs.lock().unwrap(), vec![true]);
    assert_eq!(grants(&provider), 1);
    assert_eq!(provider.check_integration()["hook_contract_current"], true);
}

#[test]
fn hooks_similar_foreign_route_is_not_claimed() {
    let foreign_entry = serde_json::json!({
        "hooks": [{ "type": "command", "command": FOREIGN_PANE_ROUTE }]
    });
    let status = serde_json::json!({
        "hooks_installed": false,
        "integration_owned": is_codeterm_hook(&foreign_entry),
        "hook_contract_current": false
    });
    assert_eq!(status["integration_owned"], false);

    let mut config = crate::config::schema::AppConfig::default();
    assert!(!infer_consent_for_owned_install(
        &mut config,
        "claude",
        &status,
        1_790_403_600,
    ));
    let provider = RepairProbeProvider::with_id("claude", status);
    assert_eq!(
        reconcile_probe(&provider, false),
        IntegrationRepairOutcome::ConsentMissing
    );
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn hooks_repaired_content_is_instance_agnostic() {
    let dir = tempfile::tempdir().unwrap();
    let dev_path = dir.path().join("dev-settings.json");
    let prod_path = dir.path().join("prod-settings.json");
    let options = IntegrationInstallOptions {
        consent: true,

        repair: true,
    };

    install_integration_at(&dev_path, &options).unwrap();
    install_integration_at(&prod_path, &options).unwrap();
    let dev_content = std::fs::read_to_string(dev_path).unwrap();
    let prod_content = std::fs::read_to_string(prod_path).unwrap();

    assert_eq!(dev_content.as_bytes(), prod_content.as_bytes());
    assert!(dev_content.contains("$CODETERM_API_URL"));
    assert!(dev_content.contains("$CODETERM_API_TOKEN"));
    assert!(dev_content.contains("$CODETERM_TAB_ID"));
    assert!(!dev_content.contains("localhost"));
    assert!(!dev_content.contains("7685"));
    assert!(!dev_content.contains("7686"));
    assert!(!dev_content.contains("CODETERM_API_PORT"));
    assert!(!dev_content.contains("Bearer ct_"));
}

#[test]
fn transport_capabilities_contract_uninstall_refuses_unowned_provider_files() {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(InstallProbeProvider::with_kind(
        "drop-in",
        IntegrationKind::PluginFile,
        Ok(()),
        serde_json::json!({"plugin_file_exists": true, "plugin_file_owned": false}),
    )));
    assert_eq!(
        super::uninstall_provider_integration_core(&registry, "drop-in"),
        Err("Domios will not remove an unmarked provider plugin file".into())
    );
    assert_eq!(
        super::uninstall_provider_integration_core(&registry, "unknown"),
        Err("Unknown provider: unknown".into())
    );
}
