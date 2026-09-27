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
use codeterm_types::notification::NotifyLevel;
use tauri::State;

use crate::service::ServiceState;
use crate::service::notification::{NotificationOrigin, PushIntent};

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
    let live_session_ids = svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| tab.agent_type.as_deref() == Some(provider_id.as_str()))
        .filter_map(|tab| tab.agent_session_id.clone())
        .collect::<Vec<_>>();
    svc.agent_registry
        .with_provider(&provider_id, |provider| {
            crate::service::integration_silence::integration_status_with_binding_fallback(
                provider.check_integration(),
                &provider_id,
                provider.has_hooks(),
                &live_session_ids,
            )
        })
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
    let (kind, status) = registry
        .with_provider(&provider_id, |provider| {
            (provider.integration_kind(), provider.check_integration())
        })
        .ok_or_else(|| format!("Unknown provider: {provider_id}"))?;
    if has_unowned_plugin_file(kind, &status) {
        return Err("CodeTerm will not change an unmarked provider plugin file".into());
    }
    let port = match api_port {
        Some(port) => port,
        None => runtime_api_port()?,
    };
    let target = status
        .get("target")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| "provider global configuration".into());
    let reported_precompact = read_precompact_choice(&status);
    let recorded_precompact = config.lock().agents.get(&provider_id).and_then(|provider| {
        provider
            .extra
            .get("precompact_memory_export")
            .and_then(toml::Value::as_bool)
    });
    let recorded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs() as i64;
    let options = crate::agent::provider::IntegrationInstallOptions {
        consent,
        precompact_memory_export: resolve_precompact_choice(
            precompact_memory_export,
            &status,
            recorded_precompact,
        ),
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
            {
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
            }
            if reported_precompact.is_some() {
                update_precompact_choice(
                    &mut updated,
                    &provider_id,
                    Some(options.precompact_memory_export),
                );
            }
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
    precompact_memory_export: Option<bool>,
) -> Result<(), String> {
    install_provider_integration_body(
        registry,
        config,
        provider_id,
        Some(api_port),
        true,
        precompact_memory_export,
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
        .with_provider(&provider_id, |provider| {
            let status = provider.check_integration();
            if has_unowned_plugin_file(provider.integration_kind(), &status) {
                return Err("CodeTerm will not remove an unmarked provider plugin file".into());
            }
            provider.uninstall_integration()
        })
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))?
}

#[tauri::command]
pub fn restore_provider_integration(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<(), String> {
    svc.agent_registry
        .with_provider(&provider_id, restore_integration_for_provider)
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))?
}

fn restore_integration_for_provider(
    provider: &dyn crate::agent::provider::AgentProvider,
) -> Result<(), String> {
    if provider.integration_kind() == crate::agent::provider::IntegrationKind::PluginFile {
        return Err("plugin-file integrations have no backup to restore".into());
    }
    provider.restore_integration()
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

/// Repairs unhealthy integrations with recorded consent or an owned install.
/// Provider capability and ownership come from provider reports.
pub fn ensure_integrations_for_all(
    registry: &crate::agent::registry::AgentRegistry,
    config: &crate::config::ConfigState,
    service: ServiceState,
) {
    for provider in registry.all_provider_arcs() {
        spawn_reconciliation(
            provider,
            config.lock().agents.clone(),
            Some(config.clone()),
            Some(service.clone()),
        );
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
            // Startup alone infers consent for existing marked installs.
            spawn_reconciliation(provider, consented.clone(), None, None);
        }
    }
}

type ProviderConfigs = std::collections::HashMap<String, crate::config::schema::ProviderConfig>;

fn recorded_flag(consented: &ProviderConfigs, id: &str, key: &str) -> Option<bool> {
    consented
        .get(id)
        .and_then(|provider| provider.extra.get(key))
        .and_then(toml::Value::as_bool)
}

fn has_unowned_plugin_file(
    kind: crate::agent::provider::IntegrationKind,
    status: &Json,
) -> bool {
    kind == crate::agent::provider::IntegrationKind::PluginFile
        && status.get("plugin_file_exists").and_then(Json::as_bool) == Some(true)
        && status.get("plugin_file_owned").and_then(Json::as_bool) != Some(true)
}

fn resolve_precompact_choice(
    requested: Option<bool>,
    status: &Json,
    recorded: Option<bool>,
) -> bool {
    requested.unwrap_or_else(|| {
        status
            .get("precompact_hook")
            .and_then(Json::as_bool)
            .or(recorded)
            .unwrap_or(false)
    })
}

fn read_precompact_choice(status: &Json) -> Option<bool> {
    status.get("precompact_hook").and_then(Json::as_bool)
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
    mut consented: ProviderConfigs,
    config: Option<crate::config::ConfigState>,
    service: Option<ServiceState>,
) {
    let id = provider.id().to_string();
    if !provider.has_installable_integration() {
        log_reconciliation(
            &id,
            &IntegrationRepairOutcome::Skipped("provider has no installable integration".into()),
            false,
        );
        return;
    }
    if !reconciling().lock().insert(id.clone()) {
        log_reconciliation(
            &id,
            &IntegrationRepairOutcome::Skipped("reconciliation is already in progress".into()),
            false,
        );
        return;
    }
    let port = runtime_api_port();
    let done = id.clone();
    let failed_id = id.clone();
    if let Err(error) = std::thread::Builder::new()
        .name(format!("integration-repair-{id}"))
        .spawn(move || {
            if let Ok(api_port) = port.as_ref() {
                provider.ensure_integration_on_startup(*api_port);
            }
            let status = provider.check_integration();
            let previously_consented = recorded_flag(
                &consented,
                &id,
                crate::config::schema::INTEGRATION_CONSENT_GRANTED,
            ) == Some(true);
            let precompact = resolve_precompact_choice(
                None,
                &status,
                recorded_flag(&consented, &id, "precompact_memory_export"),
            );
            let reported_precompact = read_precompact_choice(&status);
            let mut consent_inferred = false;
            if !previously_consented
                && crate::service::integration_silence::read_integration_owned(&status)
            {
                if let Some(config) = config.as_ref() {
                    match record_inferred_owned_consent(config, &id, &status, reported_precompact) {
                        Ok(recorded) => consent_inferred = recorded,
                        Err(error) => {
                            log_reconciliation(
                                &id,
                                &IntegrationRepairOutcome::Skipped(format!(
                                    "could not record inferred consent: {error}"
                                )),
                                false,
                            );
                            reconciling().lock().remove(&done);
                            return;
                        }
                    }
                    consented = config.lock().agents.clone();
                }
            }
            let previously_consented = recorded_flag(
                &consented,
                &id,
                crate::config::schema::INTEGRATION_CONSENT_GRANTED,
            ) == Some(true);
            if previously_consented {
                if let Some(config) = config.as_ref() {
                    if let Err(error) = record_precompact_choice(config, &id, reported_precompact) {
                        log_reconciliation(
                            &id,
                            &IntegrationRepairOutcome::Skipped(format!(
                                "could not record PreCompact choice: {error}"
                            )),
                            consent_inferred,
                        );
                        reconciling().lock().remove(&done);
                        return;
                    }
                }
            }
            let outcome = match port {
                Ok(port) => repair_integration_on_startup(
                    provider.as_ref(),
                    port,
                    &status,
                    previously_consented,
                    precompact,
                ),
                Err(error) => IntegrationRepairOutcome::Skipped(format!(
                    "runtime API port unavailable: {error}"
                )),
            };
            log_reconciliation(&id, &outcome, consent_inferred);
            if let Some(service) = service.as_ref() {
                notify_reconciliation_owner(
                    service,
                    provider.display_name(),
                    &id,
                    &status,
                    previously_consented,
                    &outcome,
                );
            }
            reconciling().lock().remove(&done);
        })
    {
        reconciling().lock().remove(&failed_id);
        log_reconciliation(
            &failed_id,
            &IntegrationRepairOutcome::Skipped(format!("repair thread failed: {error}")),
            false,
        );
    }
}

fn infer_consent_for_owned_install(
    config: &mut crate::config::schema::AppConfig,
    provider_id: &str,
    status: &Json,
    recorded_at: i64,
    precompact_memory_export: Option<bool>,
) -> bool {
    if config.agents.get(provider_id).is_some_and(|provider| {
        provider
            .extra
            .contains_key(crate::config::schema::INTEGRATION_CONSENT_GRANTED)
    }) || !crate::service::integration_silence::read_integration_owned(status)
    {
        return false;
    }
    {
        let provider = config.agents.entry(provider_id.to_string()).or_default();
        provider.extra.insert(
            crate::config::schema::INTEGRATION_CONSENT_GRANTED.into(),
            toml::Value::Boolean(true),
        );
        provider.extra.insert(
            "integration_consent_recorded_at".into(),
            toml::Value::Integer(recorded_at),
        );
        provider.extra.insert(
            "integration_consent_note".into(),
            toml::Value::String("inferred from an existing CodeTerm-owned integration".into()),
        );
    }
    update_precompact_choice(config, provider_id, precompact_memory_export);
    true
}

fn record_inferred_owned_consent(
    config: &crate::config::ConfigState,
    provider_id: &str,
    status: &Json,
    precompact_memory_export: Option<bool>,
) -> Result<bool, String> {
    let mut current = config.lock();
    let mut updated = current.clone();
    let recorded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs() as i64;
    if !infer_consent_for_owned_install(
        &mut updated,
        provider_id,
        status,
        recorded_at,
        precompact_memory_export,
    ) {
        return Ok(false);
    }
    crate::config::save_config_pub(&updated)?;
    *current = updated;
    Ok(true)
}

fn record_precompact_choice(
    config: &crate::config::ConfigState,
    provider_id: &str,
    precompact_memory_export: Option<bool>,
) -> Result<(), String> {
    let mut current = config.lock();
    let mut updated = current.clone();
    if !update_precompact_choice(&mut updated, provider_id, precompact_memory_export) {
        return Ok(());
    }
    crate::config::save_config_pub(&updated)?;
    *current = updated;
    Ok(())
}

fn update_precompact_choice(
    config: &mut crate::config::schema::AppConfig,
    provider_id: &str,
    precompact_memory_export: Option<bool>,
) -> bool {
    let Some(precompact_memory_export) = precompact_memory_export else {
        return false;
    };
    let provider = config.agents.entry(provider_id.to_string()).or_default();
    let recorded = provider
        .extra
        .get("precompact_memory_export")
        .and_then(toml::Value::as_bool);
    if recorded == Some(precompact_memory_export) {
        return false;
    }
    provider.extra.insert(
        "precompact_memory_export".into(),
        toml::Value::Boolean(precompact_memory_export),
    );
    true
}

#[derive(Debug, PartialEq, Eq)]
enum IntegrationRepairOutcome {
    Current,
    ConsentMissing,
    ForeignPluginFilePreserved,
    Repaired(Option<Json>),
    Unreadable,
    TrustGrantRequired,
    Skipped(String),
    RepairFailed(String),
}

fn reconciliation_log_line(outcome: &IntegrationRepairOutcome, consent_inferred: bool) -> String {
    let outcome = match outcome {
        IntegrationRepairOutcome::Current => "current".into(),
        IntegrationRepairOutcome::ConsentMissing => "consent missing".into(),
        IntegrationRepairOutcome::ForeignPluginFilePreserved => {
            "unmarked plugin file preserved".into()
        }
        IntegrationRepairOutcome::Repaired(Some(grant)) => {
            format!("repaired; trust grant: {grant}")
        }
        IntegrationRepairOutcome::Repaired(None) => "repaired".into(),
        IntegrationRepairOutcome::Unreadable => "unreadable".into(),
        IntegrationRepairOutcome::TrustGrantRequired => "trust required".into(),
        IntegrationRepairOutcome::Skipped(detail) => format!("skipped ({detail})"),
        IntegrationRepairOutcome::RepairFailed(detail) => format!("repair failed ({detail})"),
    };
    let inferred = if consent_inferred {
        "; consent inferred from existing CodeTerm-owned install"
    } else {
        ""
    };
    format!("integration reconciliation: {outcome}{inferred}")
}

fn log_reconciliation(
    provider_id: &str,
    outcome: &IntegrationRepairOutcome,
    consent_inferred: bool,
) {
    let line = reconciliation_log_line(outcome, consent_inferred);
    if matches!(
        outcome,
        IntegrationRepairOutcome::Unreadable | IntegrationRepairOutcome::RepairFailed(_)
    ) {
        log::warn!("[provider {provider_id}] {line}");
    } else {
        log::info!("[provider {provider_id}] {line}");
    }
}

fn reconciliation_needs_owner_notice(
    status: &Json,
    consent_recorded: bool,
    outcome: &IntegrationRepairOutcome,
) -> bool {
    let owned = crate::service::integration_silence::read_integration_owned(status);
    (owned || consent_recorded)
        && matches!(
            outcome,
            IntegrationRepairOutcome::ConsentMissing
                | IntegrationRepairOutcome::Unreadable
                | IntegrationRepairOutcome::TrustGrantRequired
                | IntegrationRepairOutcome::RepairFailed(_)
        )
}

fn notify_reconciliation_owner(
    service: &ServiceState,
    provider_name: &str,
    provider_id: &str,
    status: &Json,
    consent_recorded: bool,
    outcome: &IntegrationRepairOutcome,
) {
    if !reconciliation_needs_owner_notice(status, consent_recorded, outcome) {
        return;
    }
    let detail = match outcome {
        IntegrationRepairOutcome::RepairFailed(error) => error.as_str(),
        IntegrationRepairOutcome::Unreadable => status
            .get("hook_trust_reason")
            .and_then(Json::as_str)
            .unwrap_or("provider integration status could not be verified"),
        IntegrationRepairOutcome::TrustGrantRequired => {
            "the hook trust grant needs explicit consent"
        }
        _ => "provider integration consent is missing",
    };
    service.create_notification_ext(
        format!("{provider_name} integration needs attention"),
        Some(format!(
            "{detail}. Open Settings > Integrations and approve the provider repair, or run `codeterm integration install --provider {provider_id} --json` after consent."
        )),
        Some("warning"),
        None,
        Some(true),
        NotificationOrigin::default(),
        PushIntent {
            level: Some(NotifyLevel::ActionNeeded),
            event_key: Some(format!("provider-integration-repair:{provider_id}")),
        },
        None,
    );
}

fn repair_integration_on_startup(
    provider: &dyn crate::agent::provider::AgentProvider,
    api_port: u16,
    status: &Json,
    previously_consented: bool,
    precompact_memory_export: bool,
) -> IntegrationRepairOutcome {
    use crate::service::integration_silence::{
        authorize_reconciliation, facts_from_status, ReconcileAuthorization,
    };

    let facts = facts_from_status(
        status,
        provider.has_installable_integration(),
        previously_consented,
    );
    match authorize_reconciliation(&facts) {
        ReconcileAuthorization::AlreadyCurrent => return IntegrationRepairOutcome::Current,
        ReconcileAuthorization::Unreadable => return IntegrationRepairOutcome::Unreadable,
        ReconcileAuthorization::TrustGrantRequired => {
            return IntegrationRepairOutcome::TrustGrantRequired;
        }
        ReconcileAuthorization::ConsentMissing => {
            return IntegrationRepairOutcome::ConsentMissing;
        }
        ReconcileAuthorization::Reconcile => {}
    }
    if has_unowned_plugin_file(provider.integration_kind(), status) {
        return IntegrationRepairOutcome::ForeignPluginFilePreserved;
    }
    let options = crate::agent::provider::IntegrationInstallOptions {
        consent: true,
        precompact_memory_export,
        repair: true,
    };
    // Trust is keyed to content, so a rewrite without the grant disarms the hook.
    match crate::agent::provider::install_integration_with_trust_grant(provider, api_port, &options)
    {
        Ok(grant) => IntegrationRepairOutcome::Repaired(grant),
        Err(error) => IntegrationRepairOutcome::RepairFailed(error),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "hooks/owner_notice_tests.rs"]
mod owner_notice_tests;
