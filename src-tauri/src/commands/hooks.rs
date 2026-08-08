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
pub fn install_provider_integration(
    svc: State<'_, ServiceState>,
    config: State<'_, crate::config::ConfigState>,
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
    let status = svc
        .agent_registry
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
            svc.agent_registry
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
            svc.agent_registry
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
    use super::install_and_record_integration;
    use std::cell::Cell;

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
