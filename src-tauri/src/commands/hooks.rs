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

pub(crate) fn install_provider_integration_for_launch(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
) -> Result<(), String> {
    let api_port = runtime_api_port()?;
    registry
        .with_provider(provider_id, |provider| {
            provider.install_integration(api_port)
        })
        .ok_or_else(|| format!("Unknown provider: {provider_id}"))?
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
    provider_id: String,
    api_port: Option<u16>,
) -> Result<(), String> {
    let port = match api_port {
        Some(port) => port,
        None => runtime_api_port()?,
    };
    svc.agent_registry
        .with_provider(&provider_id, |p| p.install_integration(port))
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))?
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

/// Repairs provider integrations without holding the registry lock or blocking startup.
pub fn ensure_integrations_for_all(registry: &crate::agent::registry::AgentRegistry) {
    let port = match runtime_api_port() {
        Ok(port) => port,
        Err(error) => {
            log::warn!("[provider] integration repair skipped: {error}");
            return;
        }
    };
    for provider in registry.all_provider_arcs() {
        let id = provider.id().to_string();
        if let Err(error) = std::thread::Builder::new()
            .name(format!("integration-{id}"))
            .spawn(move || provider.ensure_integration_on_startup(port))
        {
            log::warn!("[provider {id}] integration repair thread failed: {error}");
        }
    }
}
