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
                "integration_consent_granted".into(),
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

/// Runs provider-owned read-only checks without blocking startup.
pub fn ensure_integrations_for_all(registry: &crate::agent::registry::AgentRegistry) {
    let port = match runtime_api_port() {
        Ok(port) => port,
        Err(error) => {
            log::warn!("[provider] integration check skipped: {error}");
            return;
        }
    };
    for provider in registry.all_provider_arcs() {
        let id = provider.id().to_string();
        if let Err(error) = std::thread::Builder::new()
            .name(format!("integration-check-{id}"))
            .spawn(move || provider.ensure_integration_on_startup(port))
        {
            log::warn!("[provider {id}] integration check thread failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        install_and_record_integration, install_provider_integration_body, run_install_off_runtime,
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
}
