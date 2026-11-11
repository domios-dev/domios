use super::*;

#[test]
fn manager_slots_and_the_watcher_derive_the_manager_role() {
    for value in MANAGER_TYPES.into_iter().chain([builtins::WATCHER]) {
        let parsed = RoleType::parse(value).expect("manager type parses");
        assert_eq!(parsed.role, AgentRole::Manager);
        assert_eq!(parsed.kind_id, value);
    }
}

#[test]
fn worker_names_derive_the_worker_role() {
    for value in ["worker", "qa", "reviewer", "release_manager", "checker_2"] {
        let parsed = RoleType::parse(value).expect("worker type parses");
        assert_eq!(parsed.role, AgentRole::Worker);
        assert_eq!(parsed.wire_pair(), ("Worker", value));
    }
}

#[test]
fn structural_literals_host_kinds_and_malformed_names_are_refused() {
    for value in ["Manager", "Worker", "Orchestrator", "Planner", "Watcher"] {
        assert!(matches!(
            RoleType::parse(value),
            Err(RoleTypeError::StructuralLiteral(_))
        ));
    }
    assert!(matches!(
        RoleType::parse(builtins::GENERAL_AGENT),
        Err(RoleTypeError::HostOwned(_))
    ));
    for value in ["Qa", "9lives", "release-manager", "a b"] {
        assert!(matches!(
            RoleType::parse(value),
            Err(RoleTypeError::Malformed(_))
        ));
    }
    assert_eq!(RoleType::parse("  "), Err(RoleTypeError::Empty));
}
