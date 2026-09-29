//! Per-plugin grant persistence. The powerful grants (`manage_workspaces` /
//! `spawn_agents` / `agent_commands`) need the user's decision to survive, stored
//! per-plugin as `grants.yaml` — a `{ permission: bool }` map that overrides the manifest value when present.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use codeterm_util::persisted_state::{Loaded, StateFile};

use super::manifest::PermissionsConfig;

/// The per-plugin persisted-grants file.
pub fn grants_path(bundle_dir: &Path) -> PathBuf {
    bundle_dir.join("grants.yaml")
}

const TOGGLEABLE: [&str; 5] = [
    "manage_workspaces",
    "spawn_agents",
    "agent_commands",
    "secrets",
    "analytics",
];

/// No rotating backups: the file lives inside the plugin bundle.
pub fn grants_file(bundle_dir: &Path) -> StateFile {
    StateFile::new(grants_path(bundle_dir))
        .without_backups()
        .with_validator(validate_grants)
}

pub fn parse_grants(text: &str) -> Result<HashMap<String, bool>, String> {
    if text.trim().is_empty() {
        return Ok(HashMap::new());
    }
    serde_yaml::from_str(text).map_err(|e| e.to_string())
}

pub fn validate_grants(text: &str) -> Result<(), String> {
    parse_grants(text).map(|_| ())
}

/// Read the persisted `{ permission: bool }` overrides. Missing → empty. A file
/// that fails to load revokes every toggleable permission until it loads again,
/// so a permission the user revoked never silently returns.
pub fn load_grants(bundle_dir: &Path) -> HashMap<String, bool> {
    let file = grants_file(bundle_dir);
    match file.load(parse_grants) {
        Loaded::Parsed(grants) | Loaded::Recovered { value: grants, .. } => grants,
        Loaded::Missing if file.held().is_none() => HashMap::new(),
        Loaded::Missing | Loaded::Unrecoverable { .. } => TOGGLEABLE
            .iter()
            .map(|permission| (permission.to_string(), false))
            .collect(),
    }
}

/// Apply the user's persisted grant overrides onto manifest-derived perms. A
/// present key wins over the manifest; absent keys leave the manifest value.
pub fn apply_persisted(perms: &mut PermissionsConfig, bundle_dir: &Path) {
    let g = load_grants(bundle_dir);
    if let Some(v) = g.get("manage_workspaces") {
        perms.manage_workspaces = *v;
    }
    if let Some(v) = g.get("spawn_agents") {
        perms.spawn_agents = *v;
    }
    if let Some(v) = g.get("agent_commands") {
        perms.agent_commands = *v;
    }
    if let Some(v) = g.get("secrets") {
        perms.secrets = *v;
    }
    if let Some(v) = g.get("analytics") {
        perms.analytics = *v;
    }
}

/// Persist a single grant decision (used by the install/grant flow). Merges into
/// any existing `grants.yaml`.
pub fn save_grant(bundle_dir: &Path, permission: &str, granted: bool) -> Result<(), String> {
    let mut g = load_grants(bundle_dir);
    g.insert(permission.to_string(), granted);
    let body = serde_yaml::to_string(&g).map_err(|e| format!("encode grants: {e}"))?;
    grants_file(bundle_dir)
        .save(&body)
        .map_err(|e| format!("write grants.yaml: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unparseable_grants_file_revokes_every_toggleable_permission() {
        let dir = tempfile::tempdir().unwrap();
        let broken = "spawn_agents: false
manage_workspaces: [unterminated
";
        std::fs::write(grants_path(dir.path()), broken).unwrap();
        let mut perms = PermissionsConfig {
            manage_workspaces: true,
            spawn_agents: true,
            ..Default::default()
        };

        apply_persisted(&mut perms, dir.path());

        assert!(!perms.manage_workspaces && !perms.spawn_agents);
        assert!(save_grant(dir.path(), "spawn_agents", true).is_err());
        assert!(!grants_path(dir.path()).exists());
        let copies = grants_file(dir.path()).corrupt_copies();
        assert_eq!(std::fs::read_to_string(&copies[0]).unwrap(), broken);
        let mut again = PermissionsConfig {
            spawn_agents: true,
            ..Default::default()
        };
        apply_persisted(&mut again, dir.path());
        assert!(!again.spawn_agents, "a held file stays fail-closed");
    }

    #[test]
    fn no_grants_file_leaves_manifest_perms() {
        let dir = tempfile::tempdir().unwrap();
        let mut perms = PermissionsConfig {
            manage_workspaces: true,
            ..Default::default()
        };
        apply_persisted(&mut perms, dir.path());
        assert!(perms.manage_workspaces, "manifest value preserved");
        assert!(!perms.spawn_agents);
    }

    #[test]
    fn persisted_grant_overrides_manifest() {
        let dir = tempfile::tempdir().unwrap();
        // User granted spawn_agents (manifest didn't) and revoked manage_workspaces.
        std::fs::write(
            grants_path(dir.path()),
            "spawn_agents: true\nmanage_workspaces: false\n",
        )
        .unwrap();
        let mut perms = PermissionsConfig {
            manage_workspaces: true,
            spawn_agents: false,
            ..Default::default()
        };
        apply_persisted(&mut perms, dir.path());
        assert!(perms.spawn_agents, "user grant adds the perm");
        assert!(!perms.manage_workspaces, "user revoke removes the perm");
    }

    #[test]
    fn save_grant_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        save_grant(dir.path(), "spawn_agents", true).unwrap();
        save_grant(dir.path(), "manage_workspaces", false).unwrap();
        let g = load_grants(dir.path());
        assert_eq!(g.get("spawn_agents"), Some(&true));
        assert_eq!(g.get("manage_workspaces"), Some(&false));
    }
}
