//! Generic provider-integration dispatchers.
//!
//! Each agent provider can have an optional "integration" — hooks /
//! callbacks / settings it needs to push events back into CodeTerm.
//! Claude installs JSON entries into `~/.claude/settings.json`, Codex
//! installs `~/.codex/hooks.json`, and OpenCode installs a small plugin in
//! its config directory. Providers without integrations no-op via trait
//! defaults.
//!
//! These tauri commands are thin dispatchers that route through the
//! `AgentRegistry` to the right provider implementation. Adding a new
//! provider requires zero changes here.

use serde_json::Value as Json;
use tauri::State;

use crate::service::ServiceState;

fn runtime_api_port() -> Result<u16, String> {
    let value = std::env::var("CODETERM_API_PORT")
        .map_err(|_| "CODETERM_API_PORT is missing from the running instance".to_string())?;
    value
        .parse()
        .map_err(|_| format!("CODETERM_API_PORT is invalid: {value}"))
}

#[tauri::command]
pub fn check_provider_integration(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<Json, String> {
    svc.agent_registry
        .with_provider(&provider_id, |p| p.check_integration())
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))
}

#[tauri::command]
pub async fn install_provider_integration(
    svc: State<'_, ServiceState>,
    config: State<'_, crate::config::ConfigState>,
    provider_id: String,
    api_port: Option<u16>,
    consent: bool,
    precompact_memory_export: Option<bool>,
) -> Result<(), String> {
    let registry = svc.agent_registry.clone();
    let config = config.inner().clone();
    run_install_off_runtime(move || {
        install_provider_integration_body(
            &registry,
            &config,
            provider_id,
            api_port,
            consent,
            precompact_memory_export,
        )
    })
    .await
}

/// Provider install/grant forks provider CLI subprocesses (worst case 2x20s);
/// on the Tauri async runtime that starves all IPC and freezes every tab.
async fn run_install_off_runtime<T: Send + 'static>(
    body: impl FnOnce() -> T + Send + 'static,
) -> T {
    match tokio::task::spawn_blocking(body).await {
        Ok(result) => result,
        Err(join_error) => match join_error.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(join_error) => unreachable!("install offload task is never aborted: {join_error}"),
        },
    }
}

fn install_provider_integration_body(
    registry: &crate::agent::registry::AgentRegistry,
    config: &crate::config::ConfigState,
    provider_id: String,
    api_port: Option<u16>,
    consent: bool,
    precompact_memory_export: Option<bool>,
) -> Result<(), String> {
    if !consent {
        return Err(
            "Review and consent are required before changing global provider configuration".into(),
        );
    }
    let port = match api_port {
        Some(port) => port,
        None => runtime_api_port()?,
    };
    let status = registry
        .with_provider(&provider_id, |provider| provider.check_integration())
        .ok_or_else(|| format!("Unknown provider: {provider_id}"))?;
    let target = status
        .get("target")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| "provider global configuration".into());
    let recorded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs() as i64;
    let options = crate::agent::provider::IntegrationInstallOptions {
        consent,
        precompact_memory_export: precompact_memory_export.unwrap_or(false),
        repair: false,
    };
    install_and_record_integration(
        || {
            registry
                .with_provider(&provider_id, |provider| {
                    let grant = crate::agent::provider::install_integration_with_trust_grant(
                        provider, port, &options,
                    )?;
                    if let Some(outcome) = grant {
                        log::info!("[provider {provider_id}] integration trust grant: {outcome}");
                    }
                    Ok(())
                })
                .ok_or_else(|| format!("Unknown provider: {provider_id}"))?
        },
        || {
            let mut current = config.lock();
            let mut updated = current.clone();
            let provider = updated.agents.entry(provider_id.clone()).or_default();
            provider.extra.insert(
                crate::config::schema::INTEGRATION_CONSENT_GRANTED.into(),
                toml::Value::Boolean(true),
            );
            provider.extra.insert(
                "integration_consent_recorded_at".into(),
                toml::Value::Integer(recorded_at),
            );
            provider
                .extra
                .insert("integration_target".into(), toml::Value::String(target));
            provider.extra.insert(
                "precompact_memory_export".into(),
                toml::Value::Boolean(options.precompact_memory_export),
            );
            crate::config::save_config_pub(&updated)?;
            *current = updated;
            Ok(())
        },
        || {
            registry
                .with_provider(&provider_id, |provider| provider.restore_integration())
                .ok_or_else(|| format!("Unknown provider: {provider_id}"))?
        },
    )
}

/// An explicit CLI or API install is the consent: it records consent exactly
/// like the Settings install, then installs with the trust grant.
pub fn install_provider_integration_with_consent(
    registry: &crate::agent::registry::AgentRegistry,
    config: &crate::config::ConfigState,
    provider_id: String,
    api_port: u16,
    precompact_memory_export: bool,
) -> Result<(), String> {
    install_provider_integration_body(
        registry,
        config,
        provider_id,
        Some(api_port),
        true,
        Some(precompact_memory_export),
    )
}

fn install_and_record_integration(
    install: impl FnOnce() -> Result<(), String>,
    persist: impl FnOnce() -> Result<(), String>,
    rollback: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    install()?;
    if let Err(persist_error) = persist() {
        return match rollback() {
            Ok(()) => Err(format!(
                "integration audit persistence failed; provider changes were restored: {persist_error}"
            )),
            Err(rollback_error) => Err(format!(
                "integration audit persistence failed ({persist_error}); provider rollback also failed: {rollback_error}"
            )),
        };
    }
    Ok(())
}

#[tauri::command]
pub fn uninstall_provider_integration(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<(), String> {
    svc.agent_registry
        .with_provider(&provider_id, |p| p.uninstall_integration())
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))?
}

#[tauri::command]
pub fn restore_provider_integration(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<(), String> {
    svc.agent_registry
        .with_provider(&provider_id, |p| p.restore_integration())
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))?
}

#[tauri::command]
pub fn check_provider_rule(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<Json, String> {
    svc.agent_registry
        .with_provider(&provider_id, |p| p.check_rule_status())
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))
}

#[tauri::command]
pub fn install_provider_rule(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<(), String> {
    svc.agent_registry
        .with_provider(&provider_id, |p| p.install_rule())
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))?
}

/// Repairs unhealthy integrations only when the user previously consented.
///
/// The provider capability comes from its manifest's `integration` block;
/// core deliberately never branches on a provider id.
pub fn ensure_integrations_for_all(
    registry: &crate::agent::registry::AgentRegistry,
    config: &crate::config::ConfigState,
) {
    let consented = config.lock().agents.clone();
    for provider in registry.all_provider_arcs() {
        spawn_reconciliation(provider, &consented);
    }
}

/// The same reconciliation for the one provider a plugin install or upgrade
/// just reloaded, so a machine carrying an older hook set is repaired when its
/// provider plugin updates and not only at the next daemon start.
pub fn ensure_integration_for_provider(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
) {
    let consented = crate::config::load_config().agents;
    for provider in registry.all_provider_arcs() {
        if provider.id() == provider_id {
            spawn_reconciliation(provider, &consented);
        }
    }
}

type ProviderConfigs = std::collections::HashMap<String, crate::config::schema::ProviderConfig>;

fn recorded_flag(consented: &ProviderConfigs, id: &str, key: &str) -> bool {
    consented
        .get(id)
        .and_then(|provider| provider.extra.get(key))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

/// One reconciliation per provider at a time. Daemon start and a plugin reload
/// both reach this, and the hook files are machine-global: two interleaved
/// read-modify-writes over one file is the duplication being fixed.
fn reconciling() -> &'static parking_lot::Mutex<std::collections::HashSet<String>> {
    static IN_FLIGHT: std::sync::OnceLock<parking_lot::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    IN_FLIGHT.get_or_init(Default::default)
}

fn spawn_reconciliation(
    provider: std::sync::Arc<dyn crate::agent::provider::AgentProvider>,
    consented: &ProviderConfigs,
) {
    let port = match runtime_api_port() {
        Ok(port) => port,
        Err(error) => {
            log::warn!("[provider] integration check skipped: {error}");
            return;
        }
    };
    let id = provider.id().to_string();
    let previously_consented = recorded_flag(
        consented,
        &id,
        crate::config::schema::INTEGRATION_CONSENT_GRANTED,
    );
    if !repair_is_allowed(provider.as_ref(), previously_consented) {
        return;
    }
    let precompact = recorded_flag(consented, &id, "precompact_memory_export");
    if !reconciling().lock().insert(id.clone()) {
        return;
    }
    let done = id.clone();
    if let Err(error) = std::thread::Builder::new()
        .name(format!("integration-repair-{id}"))
        .spawn(move || {
            repair_integration_on_startup(
                provider.as_ref(),
                port,
                previously_consented,
                precompact,
            );
            reconciling().lock().remove(&done);
        })
    {
        reconciling().lock().remove(&id);
        log::warn!("[provider {id}] integration repair thread failed: {error}");
    }
}

fn repair_is_allowed(
    provider: &dyn crate::agent::provider::AgentProvider,
    previously_consented: bool,
) -> bool {
    provider.has_installable_integration() && previously_consented
}

#[derive(Debug, PartialEq, Eq)]
enum IntegrationRepairOutcome {
    Current,
    Unconsented,
    Repaired,
    Unclassifiable,
    TrustGrantRequired,
}

fn repair_integration_on_startup(
    provider: &dyn crate::agent::provider::AgentProvider,
    api_port: u16,
    previously_consented: bool,
    precompact_memory_export: bool,
) -> IntegrationRepairOutcome {
    use crate::service::integration_silence::{
        authorize_reconciliation, facts_from_status, ReconcileAuthorization,
    };

    let status = provider.check_integration();
    let facts = facts_from_status(
        &status,
        provider.has_installable_integration(),
        previously_consented,
    );
    match authorize_reconciliation(&facts) {
        ReconcileAuthorization::AlreadyCurrent => return IntegrationRepairOutcome::Current,
        ReconcileAuthorization::Unreadable => {
            log::warn!(
                "[provider {}] integration reports no readable install state; nothing was changed",
                provider.id()
            );
            return IntegrationRepairOutcome::Unclassifiable;
        }
        ReconcileAuthorization::TrustGrantRequired => {
            log::info!(
                "[provider {}] hooks are untrusted and no consent is recorded; nothing was changed",
                provider.id()
            );
            return IntegrationRepairOutcome::TrustGrantRequired;
        }
        ReconcileAuthorization::ConsentMissing => {
            log::info!(
                "[provider {}] integration remains unchanged because consent was not recorded",
                provider.id()
            );
            return IntegrationRepairOutcome::Unconsented;
        }
        ReconcileAuthorization::Reconcile => {}
    }
    let options = crate::agent::provider::IntegrationInstallOptions {
        consent: true,
        precompact_memory_export,
        repair: true,
    };
    // Trust is keyed to content, so a rewrite without the grant disarms the hook.
    match crate::agent::provider::install_integration_with_trust_grant(provider, api_port, &options)
    {
        Ok(grant) => {
            log::info!(
                "[provider {}] repaired integration contract; trust grant: {}",
                provider.id(),
                grant.map_or_else(
                    || "not applicable".to_string(),
                    |outcome| outcome.to_string()
                )
            );
            IntegrationRepairOutcome::Repaired
        }
        Err(error) => {
            log::warn!(
                "[provider {}] could not classify or repair integration; existing entries were left unchanged: {error}",
                provider.id()
            );
            IntegrationRepairOutcome::Unclassifiable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        install_and_record_integration, install_provider_integration_body,
        repair_integration_on_startup, repair_is_allowed, run_install_off_runtime,
        IntegrationRepairOutcome,
    };
    use crate::agent::provider::{
        install_integration_with_trust_grant, AgentLaunchParams, AgentProvider,
        IntegrationInstallOptions, IntegrationTrustGrantCapability, ReadinessStrategy,
        TaskDelivery,
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
        install_result: Result<(), String>,
        install_thread: ThreadProbe,
        grant_outcome: Option<serde_json::Value>,
        grant_thread: ThreadProbe,
    }

    impl InstallProbeProvider {
        fn new(id: &str, install_result: Result<(), String>) -> Self {
            Self {
                id: id.to_string(),
                install_result,
                install_thread: Arc::new(StdMutex::new(None)),
                grant_outcome: None,
                grant_thread: Arc::new(StdMutex::new(None)),
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
        fn build_resume_command(
            &self,
            _session_id: &str,
            _skip_permissions: bool,
        ) -> Option<String> {
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
        fn install_integration(
            &self,
            _api_port: u16,
            _options: &IntegrationInstallOptions,
        ) -> Result<(), String> {
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
        repairs: StdMutex<Vec<bool>>,
        grant_calls: std::sync::atomic::AtomicUsize,
        error: Option<String>,
        live_session: StdMutex<String>,
    }

    impl RepairProbeProvider {
        fn new(status: serde_json::Value) -> Self {
            Self {
                id: "fixture-provider".into(),
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
        fn build_resume_command(
            &self,
            _session_id: &str,
            _skip_permissions: bool,
        ) -> Option<String> {
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
        repair_integration_on_startup(&provider, 7686, true, false);
        assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            provider.check_integration()["session_start_route"],
            "tab-session-start"
        );
    }

    #[test]
    fn hook_contract_repair_is_idempotent_by_write_count() {
        let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
        repair_integration_on_startup(&provider, 7686, true, false);
        repair_integration_on_startup(&provider, 7686, true, false);
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
        repair_integration_on_startup(&provider, 7686, false, false);
        assert!(provider.consents.lock().unwrap().is_empty());
        assert_eq!(grants(&provider), 0);
        repair_integration_on_startup(&provider, 7686, true, false);
        assert_eq!(*provider.consents.lock().unwrap(), vec![true]);
        assert_eq!(*provider.repairs.lock().unwrap(), vec![true]);
        assert_eq!(grants(&provider), 1);
    }

    #[test]
    fn untrusted_hash_with_recorded_consent_is_rewritten_and_regranted() {
        let provider =
            RepairProbeProvider::new(serde_json::json!({ "integration_state": "untrusted_hash" }));
        let outcome = repair_integration_on_startup(&provider, 7686, true, false);
        assert_eq!(outcome, IntegrationRepairOutcome::Repaired);
        assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(grants(&provider), 1);
    }

    #[test]
    fn untrusted_hash_without_consent_is_left_untouched() {
        let provider =
            RepairProbeProvider::new(serde_json::json!({ "integration_state": "untrusted_hash" }));
        let outcome = repair_integration_on_startup(&provider, 7686, false, false);
        assert_eq!(outcome, IntegrationRepairOutcome::TrustGrantRequired);
        assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(provider.consents.lock().unwrap().is_empty());
        assert_eq!(grants(&provider), 0);
    }

    #[test]
    fn failed_repair_grants_no_trust() {
        let provider =
            RepairProbeProvider::unclassifiable(serde_json::json!({ "hooks_installed": false }));
        repair_integration_on_startup(&provider, 7686, true, false);
        assert_eq!(grants(&provider), 0);
    }

    #[test]
    fn hook_contract_repair_preserves_live_session() {
        let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
        let before = provider.live_session.lock().unwrap().clone();
        repair_integration_on_startup(&provider, 7686, true, false);
        assert_eq!(*provider.live_session.lock().unwrap(), before);
    }

    #[test]
    fn hook_contract_repair_is_capability_driven_for_non_claude_fixture() {
        let provider = RepairProbeProvider::new(serde_json::json!({ "hooks_installed": false }));
        assert_ne!(provider.id(), "claude");
        assert!(repair_is_allowed(&provider, true));
        assert!(!repair_is_allowed(&provider, false));
        repair_integration_on_startup(&provider, 7686, true, false);
        assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn hook_contract_repair_leaves_unclassifiable_entry_and_reports_outcome() {
        let provider =
            RepairProbeProvider::unclassifiable(serde_json::json!({ "hooks_installed": false }));
        let outcome = repair_integration_on_startup(&provider, 7686, true, false);
        assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(*provider.consents.lock().unwrap(), vec![true]);
        assert!(matches!(outcome, IntegrationRepairOutcome::Unclassifiable));
    }
}
