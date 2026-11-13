use super::*;

#[test]
fn only_the_watcher_has_a_kind_without_a_team_registration() {
    let watcher = RoleName::parse(builtins::WATCHER).expect("watcher parses");
    assert_eq!(
        watcher.host_owned(),
        Some(RoleType::new(AgentRole::Manager, builtins::WATCHER))
    );
    for value in [
        "orchestrator",
        "planner",
        "worker",
        "qa",
        "security_lead",
        "checker_2",
    ] {
        let parsed = RoleName::parse(value).expect("role name parses");
        assert_eq!(parsed.as_str(), value);
        assert_eq!(parsed.host_owned(), None, "{value}");
    }
}

#[test]
fn reserved_manager_names_are_never_workers() {
    for value in builtins::BUILTIN_MANAGERS.into_iter().chain([builtins::WATCHER]) {
        assert!(RoleName::parse(value).unwrap().is_manager_name(), "{value}");
    }
    for value in ["worker", "qa", "security_lead"] {
        assert!(!RoleName::parse(value).unwrap().is_manager_name(), "{value}");
    }
}

#[test]
fn structural_literals_host_kinds_and_malformed_names_are_refused() {
    for value in ["Manager", "Worker", "Orchestrator", "Planner", "Watcher"] {
        assert!(matches!(
            RoleName::parse(value),
            Err(RoleTypeError::StructuralLiteral(_))
        ));
    }
    assert!(matches!(
        RoleName::parse(builtins::GENERAL_AGENT),
        Err(RoleTypeError::HostOwned(_))
    ));
    for value in ["Qa", "9lives", "release-manager", "a b"] {
        assert!(matches!(
            RoleName::parse(value),
            Err(RoleTypeError::Malformed(_))
        ));
    }
    assert_eq!(RoleName::parse("  "), Err(RoleTypeError::Empty));
}

#[test]
fn a_resolved_type_launches_as_its_registered_kind() {
    let worker = RoleType::new(AgentRole::Worker, "checker");
    assert!(worker.is_worker());
    assert_eq!(worker.wire_pair(), ("Worker", "checker"));
    let manager = RoleType::new(AgentRole::Manager, "security_lead");
    assert!(!manager.is_worker());
    assert_eq!(manager.wire_pair(), ("Manager", "security_lead"));
}
