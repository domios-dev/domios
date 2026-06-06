//! Generic provider-integration dispatchers.
//!
//! Each agent provider can have an optional "integration" — hooks /
//! callbacks / settings it needs to push events back into CodeTerm.
//! Claude installs JSON entries into `~/.claude/settings.json`. Codex
//! and Opencode are observed entirely through PTY parsing and need no
//! integration at all (trait defaults give them `status: "ok"`).
//!
//! These tauri commands are thin dispatchers that route through the
//! `AgentRegistry` to the right provider implementation. Adding a new
//! provider requires zero changes here.

use serde_json::Value as Json;
use tauri::State;

use crate::service::ServiceState;

fn default_api_port() -> u16 {
    #[cfg(debug_assertions)]
    {
        7686
    }
    #[cfg(not(debug_assertions))]
    {
        7685
    }
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
    let port = api_port.unwrap_or_else(default_api_port);
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

/// Called once at startup. Iterates every registered provider and gives
/// it a chance to auto-install its CodeTerm integration if missing.
/// Providers without an integration (Codex, Opencode) just no-op.
pub fn ensure_integrations_for_all(registry: &crate::agent::registry::AgentRegistry) {
    let port = default_api_port();
    let ids = registry.provider_ids();
    for id in &ids {
        registry.with_provider(id, |p| p.ensure_integration_on_startup(port));
    }
}
