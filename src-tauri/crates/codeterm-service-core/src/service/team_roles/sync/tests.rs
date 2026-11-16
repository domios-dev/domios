use super::*;
use crate::service::persona::{
    self,
    occasion::{compose, Occasion},
};
use codeterm_types::agent::AgentRole;

fn manager(team: &str, text: &str) -> RoleRegistration {
    let mut role = crate::service::builtin_roles::default_registrations(team)
        .into_iter()
        .find(|role| role.name == "curator")
        .unwrap();
    role.instructions = text.into();
    role
}

#[test]
fn role_sync_preserves_fields_roles_persistence_and_next_launch_with_noop_replay() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    let mut old = manager("team_a", "opaque-before\n");
    old.reasoning = Some("high".into());
    old.skills = vec!["opaque-skill".into()];
    old.label = "opaque-label".into();
    old.model = None;
    old.description = Some("opaque-description".into());
    old.files = vec![codeterm_types::team::role_registration::RoleFileRef {
        path: "opaque-file".into(),
        description: Some("opaque-file-description".into()),
    }];
    old.consent_decision_id = Some("opaque-original-consent".into());
    let mut other = manager("team_a", "opaque-other");
    other.name = "planner".into();
    store.register(old.clone()).unwrap();
    store.register(other.clone()).unwrap();
    let next = manager("team_a", "opaque-after\nsecond-fragment\n");
    let preview = store.sync_preview("team_a", "curator", &next).unwrap();
    assert_eq!(preview.current_hash, hash("opaque-before\n"));
    assert_eq!(
        preview.replacement_hash,
        hash("opaque-after\nsecond-fragment\n")
    );
    assert!(preview
        .diff
        .contains("-opaque-before\n+opaque-after\n+second-fragment\n"));
    assert!(store
        .sync_instructions(
            "team_a",
            "curator",
            &preview.current_hash,
            &preview.replacement_hash,
            &next,
            5
        )
        .unwrap());
    let mut expected = old;
    expected.instructions = next.instructions;
    assert_eq!(
        store.registrations("team_a"),
        vec![expected.clone(), other.clone()]
    );
    assert_eq!(
        TeamRoleStore::new(root.path()).registrations("team_a"),
        vec![expected.clone(), other]
    );
    let preset = persona::resolve(
        &store.registrations("team_a"),
        Some(AgentRole::Manager),
        Some("curator"),
        None,
    )
    .unwrap();
    let emitted = compose(
        Occasion::LaunchAssignment,
        Some(&preset),
        Some("opaque-task"),
    )
    .unwrap();
    assert!(emitted.contains("opaque-after\nsecond-fragment\n"));
    assert!(!emitted.contains("opaque-before"));
    let decision_path = root.path().join("team_a/role-sync-curator.json");
    let audit = std::fs::read(&decision_path).unwrap();
    let decision: serde_json::Value = serde_json::from_slice(&audit).unwrap();
    assert_eq!(decision["team_id"], "team_a");
    assert_eq!(decision["role"], "curator");
    assert_eq!(decision["current_hash"], preview.current_hash);
    assert_eq!(decision["replacement_hash"], preview.replacement_hash);
    assert_eq!(
        decision["source"],
        serde_json::to_value(preview.source).unwrap()
    );
    assert_eq!(decision["approved_at"], 5);
    let bytes = std::fs::read(store.path("team_a")).unwrap();
    assert!(!store
        .sync_instructions(
            "team_a",
            "curator",
            &preview.current_hash,
            &preview.replacement_hash,
            &expected,
            99
        )
        .unwrap());
    assert_eq!(std::fs::read(store.path("team_a")).unwrap(), bytes);
    assert_eq!(std::fs::read(decision_path).unwrap(), audit);
}

#[test]
fn role_sync_refuses_custom_packages_workers_and_stale_current_or_source_hashes() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    let old = manager("team_a", "opaque-old");
    store.register(old.clone()).unwrap();
    let source = manager("team_a", "opaque-new");
    for (current, replacement) in [
        ("stale".into(), hash(&source.instructions)),
        (hash(&old.instructions), "stale".into()),
    ] {
        assert!(store
            .sync_instructions("team_a", "curator", &current, &replacement, &source, 5)
            .is_err());
    }
    assert_eq!(store.registrations("team_a"), vec![old]);
    let mut custom = manager("team_b", "opaque-custom");
    custom.package.name = "com.example/custom".into();
    store.register(custom).unwrap();
    assert!(store
        .sync_preview("team_b", "curator", &manager("team_b", "opaque-new"))
        .is_err());
    let mut worker = manager("team_c", "opaque-worker");
    worker.kind = RoleKind::Worker;
    worker.name = "worker".into();
    store.register(worker.clone()).unwrap();
    assert!(store.sync_preview("team_c", "worker", &worker).is_err());
    assert!(!root.path().join("team_a/role-sync-curator.json").exists());
}

#[test]
fn role_sync_save_failure_preserves_cached_and_persisted_instructions() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    let old = manager("team_a", "opaque-old");
    store.register(old.clone()).unwrap();
    let source = manager("team_a", "opaque-new");
    let path = store.path("team_a");
    let before = std::fs::read(&path).unwrap();
    assert!(store
        .sync_instructions_with(
            "team_a",
            "curator",
            (&hash(&old.instructions), &hash(&source.instructions)),
            &source,
            5,
            |_, _| Err("opaque-save-failure".into())
        )
        .is_err());
    assert_eq!(store.registrations("team_a"), vec![old.clone()]);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(root.path().join("team_a/role-sync-curator.json")).unwrap();
    std::fs::create_dir(root.path().join("team_a/role-sync-curator.json")).unwrap();
    assert!(store
        .sync_instructions(
            "team_a",
            "curator",
            &hash(&old.instructions),
            &hash(&source.instructions),
            &source,
            5
        )
        .is_err());
    assert_eq!(store.registrations("team_a"), vec![old]);
    assert_eq!(std::fs::read(path).unwrap(), before);
}
