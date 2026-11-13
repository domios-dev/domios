use codeterm_team_package::{RoleKind, RoleStart, StartMode};
use codeterm_types::agent::AgentRole;
use codeterm_types::team::role_registration::PinnedPackage;

use super::*;

fn worker(team_id: &str, name: &str, instructions: &str) -> RoleRegistration {
    RoleRegistration {
        team_id: team_id.into(),
        kind: RoleKind::Worker,
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

fn manager(team_id: &str, name: &str) -> RoleRegistration {
    RoleRegistration {
        kind: RoleKind::Manager,
        ..worker(team_id, name, "fixture-prose")
    }
}

#[test]
fn registration_refuses_host_owned_names_reserved_workers_and_path_escapes() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    for name in [
        builtins::ORCHESTRATOR,
        builtins::PLANNER,
        builtins::WATCHER,
        builtins::GENERAL_AGENT,
    ] {
        assert!(
            store.register(worker("team_a", name, "x")).is_err(),
            "{name}"
        );
    }
    for host_owned in [builtins::WATCHER, builtins::GENERAL_AGENT] {
        assert!(store.register(manager("team_a", host_owned)).is_err(), "{host_owned}");
    }
    assert!(store.register(worker("../escape", "checker", "x")).is_err());
    assert!(store.register(worker("team_a", "checker", "  ")).is_err());
    assert!(!root.path().join("team_a").exists());
}

#[test]
fn a_custom_manager_registers_and_resolves_as_a_manager() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    store.register(manager("team_a", builtins::ORCHESTRATOR)).unwrap();
    assert_eq!(
        store.register(manager("team_a", "security_lead")).unwrap(),
        RoleRegistrationOutcome::Created
    );
    assert_eq!(
        store.role_type(Some("team_a"), "security_lead").unwrap(),
        RoleType::new(AgentRole::Manager, "security_lead")
    );
    assert_eq!(
        store.worker_type(Some("team_a"), Some("security_lead")),
        Err(RoleTypeError::NotAWorker {
            name: "security_lead".into(),
            workers: Vec::new()
        })
    );
}

#[test]
fn a_team_has_exactly_its_roles_and_refuses_unlisted_names() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    store.register(manager("team_a", builtins::ORCHESTRATOR)).unwrap();
    store
        .register(worker("team_a", "checker", "fixture-prose"))
        .unwrap();
    assert_eq!(
        store.role_type(Some("team_a"), "checker").unwrap(),
        RoleType::new(AgentRole::Worker, "checker")
    );
    for unlisted in [builtins::QA, builtins::PLANNER, "scout"] {
        assert!(
            matches!(
                store.role_type(Some("team_a"), unlisted),
                Err(RoleTypeError::NotRegistered { .. })
            ),
            "{unlisted}"
        );
    }
    assert_eq!(
        store.worker_type(Some("team_a"), Some(builtins::QA)),
        Err(RoleTypeError::NotRegistered {
            name: builtins::QA.into(),
            roles: vec!["checker".into()]
        })
    );
    assert_eq!(
        store.worker_type(Some("team_a"), Some(builtins::PLANNER)),
        Err(RoleTypeError::NotAWorker {
            name: builtins::PLANNER.into(),
            workers: vec!["checker".into()]
        })
    );
    assert_eq!(
        store.role_type(Some("team_a"), builtins::WATCHER).unwrap(),
        RoleType::new(AgentRole::Manager, builtins::WATCHER)
    );
    assert!(matches!(
        store.role_type(Some("team_a"), "Manager"),
        Err(RoleTypeError::StructuralLiteral(_))
    ));
}

#[test]
fn dispatch_without_a_role_uses_worker_only_when_the_team_lists_it() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    store.register(manager("team_a", builtins::ORCHESTRATOR)).unwrap();
    store
        .register(worker("team_a", "checker", "fixture-prose"))
        .unwrap();
    assert_eq!(
        store.worker_type(Some("team_a"), None),
        Err(RoleTypeError::NoDefaultWorker {
            workers: vec!["checker".into()]
        })
    );
    store
        .register(worker("team_a", builtins::WORKER, "fixture-prose"))
        .unwrap();
    assert_eq!(
        store.worker_type(Some("team_a"), None).unwrap(),
        RoleType::new(AgentRole::Worker, builtins::WORKER)
    );
    // A Team with no registrations is the default package, which lists `worker`.
    assert_eq!(
        store.worker_type(Some("team_b"), None).unwrap(),
        RoleType::new(AgentRole::Worker, builtins::WORKER)
    );
    assert_eq!(
        store.role_type(Some("team_b"), builtins::QA).unwrap(),
        RoleType::new(AgentRole::Worker, builtins::QA)
    );
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
        start: RoleStart::Schedule {
            schedule: schedule.into(),
        },
        ..manager("team_a", "curator")
    }
}

#[test]
fn a_curator_schedule_becomes_a_window_inside_the_host_bounds() {
    use codeterm_types::plan::CuratorWindow;
    assert_eq!(window_of(&[curator("45m..2h")]), Some(CuratorWindow::HOST));
    assert_eq!(
        window_of(&[curator("1h..90m")]),
        Some(CuratorWindow {
            min_secs: 3600,
            max_secs: 5400
        })
    );
    assert_eq!(window_of(&[curator("5m..12h")]), Some(CuratorWindow::HOST));
}

#[test]
fn a_team_without_a_scheduled_curator_manager_has_no_curator_rounds() {
    assert_eq!(window_of(&[]), None);
    assert_eq!(window_of(&[manager("team_a", builtins::ORCHESTRATOR)]), None);
    let on_demand = RoleRegistration {
        start: RoleStart::Mode(StartMode::OnDemand),
        ..curator("45m..2h")
    };
    assert_eq!(window_of(&[on_demand]), None);
    // A worker or a custom manager with a schedule never schedules Curator rounds.
    let scheduled_worker = RoleRegistration {
        start: RoleStart::Schedule {
            schedule: "45m..2h".into(),
        },
        ..worker("team_a", "checker", "fixture-prose")
    };
    let scheduled_custom = RoleRegistration {
        start: RoleStart::Schedule {
            schedule: "45m..2h".into(),
        },
        ..manager("team_a", "auditor")
    };
    assert_eq!(window_of(&[scheduled_worker, scheduled_custom]), None);
    // A Team with no registrations keeps the default package's curator rounds.
    let default = crate::service::builtin_roles::default_registrations("team_a");
    assert_eq!(window_of(&default), Some(codeterm_types::plan::CuratorWindow::HOST));
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
            kind: RoleKind::Manager,
            label: "Conductor".into(),
            ..worker(&team, "orchestrator", "fixture-prose")
        },
        RoleRegistration {
            label: "Field Scout".into(),
            ..worker(&team, "scout", "fixture-prose")
        },
    ]);
    stamp_team_with(&mux, &team, &source);
    // Read back through the projection `/api/tabs` serves, not the in-memory tab.
    let summary =
        crate::dispatch::build_tab_summary_json(&mux.read().snapshot(), None, None, false, false);
    let projected = |tab_id: &str| {
        summary
            .iter()
            .find(|row| row["id"] == tab_id)
            .and_then(|row| row["role_label"].as_str())
            .map(str::to_string)
    };
    assert_eq!(projected(&anchor).as_deref(), Some("Conductor"));
    assert_eq!(projected(&scout).as_deref(), Some("Field Scout"));
}
