use codeterm_team_package::{RoleStart, StartMode};
use codeterm_types::agent::AgentRole;
use codeterm_types::team::role_registration::PinnedPackage;

use super::*;

fn worker(team_id: &str, name: &str, instructions: &str) -> RoleRegistration {
    RoleRegistration {
        team_id: team_id.into(),
        group: TeamRoleGroup::Worker,
        name: name.into(),
        label: name.into(),
        description: None,
        start: RoleStart::Mode(StartMode::OnDemand),
        package: PinnedPackage {
            name: "com.example/fixture".into(),
            version: "1.0.0".into(),
        },
        instructions: instructions.into(),
        model: None,
        reasoning: None,
        placement_peer: None,
        consent_decision_id: None,
        files: Vec::new(),
        skills: Vec::new(),
        registered_at: 7,
    }
}

#[test]
fn registrations_persist_across_a_restart_and_stay_pinned() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    let first = worker("team_a", "checker", "fixture-prose-one");
    assert_eq!(
        store.register(first.clone()).unwrap(),
        RoleRegistrationOutcome::Created
    );
    let mut again = first.clone();
    again.registered_at = 99;
    assert_eq!(
        store.register(again).unwrap(),
        RoleRegistrationOutcome::AlreadyPresent
    );
    assert_eq!(
        store
            .register(worker("team_a", "checker", "fixture-prose-two"))
            .unwrap(),
        RoleRegistrationOutcome::Pinned
    );

    let restarted = TeamRoleStore::new(root.path());
    assert_eq!(restarted.registrations("team_a"), vec![first]);
    assert!(restarted.path("team_a").is_file());
}

#[test]
fn a_team_without_registrations_resolves_the_default_package() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    let team = store.registrations("team_restored_before_registry");
    assert!(team.is_empty());
    let preset = crate::service::persona::resolve(
        &team,
        Some(AgentRole::Manager),
        Some(builtins::ORCHESTRATOR),
        None,
    )
    .unwrap();
    assert!(matches!(
        preset.provenance(),
        Some(codeterm_types::team::role_registration::RoleProvenance::Builtin { .. })
    ));
}

#[test]
fn registration_refuses_reserved_names_and_path_escapes() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    for name in [
        builtins::ORCHESTRATOR,
        builtins::WATCHER,
        builtins::GENERAL_AGENT,
    ] {
        assert!(
            store.register(worker("team_a", name, "x")).is_err(),
            "{name}"
        );
    }
    let mut manager = worker("team_a", "checker", "x");
    manager.group = TeamRoleGroup::Manager;
    assert!(store.register(manager).is_err());
    assert!(store.register(worker("../escape", "checker", "x")).is_err());
    assert!(store.register(worker("team_a", "checker", "  ")).is_err());
    assert!(!root.path().join("team_a").exists());
}

#[test]
fn role_types_name_manager_slots_or_workers_available_to_the_team() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    store
        .register(worker("team_a", "checker", "fixture-prose"))
        .unwrap();
    assert_eq!(
        store.role_type(Some("team_a"), "checker").unwrap().role,
        AgentRole::Worker
    );
    assert_eq!(
        store.role_type(None, "planner").unwrap().role,
        AgentRole::Manager
    );
    assert!(store.role_type(None, "qa").is_ok());
    assert_eq!(
        store.role_type(Some("team_b"), "checker"),
        Err(RoleTypeError::UnavailableWorker("checker".into()))
    );
    assert!(matches!(
        store.role_type(Some("team_a"), "Manager"),
        Err(RoleTypeError::StructuralLiteral(_))
    ));
}

#[test]
fn reload_rereads_registrations_written_by_another_store() {
    let root = tempfile::tempdir().unwrap();
    let reader = TeamRoleStore::new(root.path());
    assert!(reader.registrations("team_a").is_empty());
    TeamRoleStore::new(root.path())
        .register(worker("team_a", "checker", "fixture-prose"))
        .unwrap();
    assert!(reader.registrations("team_a").is_empty());
    reader.reload(Some("team_a"));
    assert_eq!(reader.registrations("team_a").len(), 1);
}
