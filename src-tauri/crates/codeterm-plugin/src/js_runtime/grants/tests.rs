use super::*;

#[test]
fn lark_recording_contract_requires_explicit_persisted_grant_even_for_manifest_request() {
    let dir = tempfile::tempdir().unwrap();
    let mut perms = PermissionsConfig {
        local_recording: true,
        ..Default::default()
    };
    apply_persisted(&mut perms, dir.path());
    assert!(!perms.local_recording);
    save_grant(dir.path(), "local_recording", true).unwrap();
    apply_persisted(&mut perms, dir.path());
    assert!(perms.local_recording);
    save_grant(dir.path(), "local_recording", false).unwrap();
    apply_persisted(&mut perms, dir.path());
    assert!(!perms.local_recording);
}

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
    assert_eq!(g.permissions.get("spawn_agents"), Some(&true));
    assert_eq!(g.permissions.get("manage_workspaces"), Some(&false));
}

#[test]
fn user_network_grants_merge_into_the_effective_allowlist() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        grants_path(dir.path()),
        "network_allow:\n  - api.example.test:8443\n",
    )
    .unwrap();
    let mut perms = PermissionsConfig::default();

    apply_persisted(&mut perms, dir.path());

    assert_eq!(perms.network.allow, ["api.example.test:8443"]);
}
