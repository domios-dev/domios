//! Per-plugin grant persistence. The powerful grants (`manage_workspaces` /
//! `spawn_agents` / `agent_commands`) need the user's decision to survive, stored
//! per-plugin in `grants.yaml` alongside exact-host network additions.

use std::path::{Path, PathBuf};

use codeterm_util::persisted_state::{Loaded, StateFile};
use codeterm_util::plugin_grants::PluginGrants;

use super::manifest::PermissionsConfig;

/// The per-plugin persisted-grants file.
pub fn grants_path(bundle_dir: &Path) -> PathBuf {
    bundle_dir.join("grants.yaml")
}

const TOGGLEABLE: [&str; 8] = [
    "project_write",
    "manage_workspaces",
    "spawn_agents",
    "agent_commands",
    "surfaces",
    "secrets",
    "analytics",
    "local_recording",
];

/// No rotating backups: the file lives inside the plugin bundle.
pub fn grants_file(bundle_dir: &Path) -> StateFile {
    StateFile::new(grants_path(bundle_dir))
        .with_private_permissions()
        .without_backups()
        .with_validator(validate_grants)
}

pub fn parse_grants(text: &str) -> Result<PluginGrants, String> {
    if text.trim().is_empty() {
        return Ok(PluginGrants::default());
    }
    let grants: PluginGrants = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    for host in &grants.network_allow {
        codeterm_util::network_host::normalize_network_host(host)?;
    }
    Ok(grants)
}

pub fn validate_grants(text: &str) -> Result<(), String> {
    parse_grants(text).map(|_| ())
}

/// Read persisted permission overrides and network additions. Missing → empty. A file
/// that fails to load revokes every toggleable permission until it loads again,
/// so a permission the user revoked never silently returns.
pub fn load_grants(bundle_dir: &Path) -> PluginGrants {
    let file = grants_file(bundle_dir);
    match file.load(parse_grants) {
        Loaded::Parsed(grants) | Loaded::Recovered { value: grants, .. } => grants,
        Loaded::Missing if file.held().is_none() => PluginGrants::default(),
        Loaded::Missing | Loaded::Unrecoverable { .. } => PluginGrants {
            permissions: TOGGLEABLE
                .iter()
                .map(|permission| (permission.to_string(), false))
                .collect(),
            network_allow: Vec::new(),
            project_write_roots: Vec::new(),
        },
    }
}

/// Apply the user's persisted grant overrides onto manifest-derived perms. A
/// present key wins over the manifest; absent keys leave the manifest value.
pub fn apply_persisted(perms: &mut PermissionsConfig, bundle_dir: &Path) {
    let g = load_grants(bundle_dir);
    perms.local_recording = g.permissions.get("local_recording") == Some(&true);
    if let Some(v) = g.permissions.get("manage_workspaces") {
        perms.manage_workspaces = *v;
    }
    if let Some(v) = g.permissions.get("spawn_agents") {
        perms.spawn_agents = *v;
    }
    if let Some(v) = g.permissions.get("agent_commands") {
        perms.agent_commands = *v;
    }
    if let Some(v) = g.permissions.get("surfaces") {
        perms.surfaces = *v;
    }
    if let Some(v) = g.permissions.get("secrets") {
        perms.secrets = *v;
    }
    if let Some(v) = g.permissions.get("analytics") {
        perms.analytics = *v;
    }
    if let Some(v) = g.permissions.get("graph_events") {
        perms.graph_events = *v;
    }
    for host in g.network_allow {
        if !perms.network.allow.contains(&host) {
            perms.network.allow.push(host);
        }
    }
}

/// Persist one permission decision while preserving other grants.
pub fn save_grant(bundle_dir: &Path, permission: &str, granted: bool) -> Result<(), String> {
    let mut g = load_grants(bundle_dir);
    g.permissions.insert(permission.to_string(), granted);
    let body = serde_yaml::to_string(&g).map_err(|e| format!("encode grants: {e}"))?;
    grants_file(bundle_dir)
        .save(&body)
        .map_err(|e| format!("write grants.yaml: {e}"))
}

#[cfg(test)]
mod tests;
