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

fn curator(schedule: &str) -> RoleRegistration {
    RoleRegistration {
        group: TeamRoleGroup::Manager,
        name: "curator".into(),
        start: RoleStart::Schedule {
            schedule: schedule.into(),
        },
        ..worker("team_a", "curator", "fixture-prose")
    }
}

#[test]
fn a_curator_schedule_becomes_a_window_inside_the_host_bounds() {
    use codeterm_types::plan::CuratorWindow;
    assert_eq!(window_of(&[]), CuratorWindow::HOST);
    assert_eq!(window_of(&[curator("45m..2h")]), CuratorWindow::HOST);
    assert_eq!(
        window_of(&[curator("1h..90m")]),
        CuratorWindow {
            min_secs: 3600,
            max_secs: 5400
        }
    );
    assert_eq!(window_of(&[curator("5m..12h")]), CuratorWindow::HOST);
    // Workers never schedule the Curator.
    assert_eq!(
        window_of(&[worker("team_a", "checker", "fixture-prose")]),
        CuratorWindow::HOST
    );
}

#[test]
fn a_tab_carries_a_declared_or_custom_role_label_and_builtins_keep_theirs() {
    let custom = worker("team_a", "scout", "fixture-prose");
    let builtin = worker("team_a", "qa", "fixture-prose");
    let named = RoleRegistration {
        label: "Gatekeeper".into(),
        ..worker("team_a", "reviewer", "fixture-prose")
    };
    let team = [custom, builtin, named];
    assert_eq!(
        role_label_for(&team, AgentRole::Worker, "scout").as_deref(),
        Some("scout")
    );
    assert_eq!(role_label_for(&team, AgentRole::Worker, "qa"), None);
    assert_eq!(
        role_label_for(&team, AgentRole::Worker, "reviewer").as_deref(),
        Some("Gatekeeper")
    );
    assert_eq!(
        role_label_for(&team, AgentRole::Worker, "unregistered"),
        None
    );
}

#[test]
fn forgetting_a_team_drops_its_registrations_and_backups() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    for name in ["checker", "scout"] {
        store
            .register(worker("team_a", name, "fixture-prose"))
            .unwrap();
    }
    store.forget("team_a").unwrap();
    assert!(store.registrations("team_a").is_empty());
    assert!(!store.path("team_a").exists());
    assert!(TeamRoleStore::new(root.path())
        .registrations("team_a")
        .is_empty());
    store.forget("team_never_registered").unwrap();
}

#[test]
fn labels_land_once_the_team_and_its_registrations_exist() {
    use crate::multiplexer::state::MultiplexerState;
    use codeterm_util::lock_hold::TimedRwLock;

    let (mut state, anchor) = MultiplexerState::new();
    let workspace = state.find_workspace_for_tab(&anchor).unwrap();
    let scout = state.create_tab(&workspace).unwrap();
    state.set_orchestrator_role(&anchor).unwrap();
    let mux: crate::multiplexer::commands::MuxState =
        std::sync::Arc::new(TimedRwLock::new("mux-test", state));
    let registered = std::sync::Mutex::new(Vec::<RoleRegistration>::new());
    let source = |team: Option<&str>| -> Vec<RoleRegistration> {
        let all = registered.lock().unwrap();
        all.iter()
            .filter(|registration| Some(registration.team_id.as_str()) == team)
            .cloned()
            .collect()
    };

    stamp_with(&mux, &anchor, source);
    assert_eq!(mux.read().get_tab(&anchor).unwrap().role_label, None);

    mux.write()
        .set_tab_role_kind(&scout, AgentRole::Worker, Some("scout"), Some(&anchor))
        .unwrap();
    let team = mux
        .read()
        .get_tab(&anchor)
        .unwrap()
        .team_id
        .clone()
        .unwrap();
    registered.lock().unwrap().extend([
        RoleRegistration {
            group: TeamRoleGroup::Manager,
            label: "Conductor".into(),
            ..worker(&team, "orchestrator", "fixture-prose")
        },
        RoleRegistration {
            label: "Field Scout".into(),
            ..worker(&team, "scout", "fixture-prose")
        },
    ]);
    stamp_team_with(&mux, &team, &source);
    let mux = mux.read();
    assert_eq!(
        mux.get_tab(&anchor).unwrap().role_label.as_deref(),
        Some("Conductor")
    );
    assert_eq!(
        mux.get_tab(&scout).unwrap().role_label.as_deref(),
        Some("Field Scout")
    );
}
