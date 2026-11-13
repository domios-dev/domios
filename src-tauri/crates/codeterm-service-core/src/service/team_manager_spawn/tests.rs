use super::*;

fn facts() -> ManagerSpawnFacts {
    ManagerSpawnFacts {
        name: "security_lead".into(),
        orchestrator_team: Some("team_a".into()),
        registered: true,
        live: false,
        task_bound: false,
    }
}

fn code(result: Result<String, String>) -> String {
    result
        .unwrap_err()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn the_orchestrator_starts_a_registered_custom_manager_in_its_team() {
    assert_eq!(admit(&facts()), Ok("team_a".into()));
}

#[test]
fn only_the_team_orchestrator_starts_a_custom_manager() {
    let refused = admit(&ManagerSpawnFacts {
        orchestrator_team: None,
        ..facts()
    });
    assert_eq!(code(refused), TEAM_ROLE_REQUIRES_ORCHESTRATOR);
}

#[test]
fn a_second_live_instance_per_name_is_refused() {
    let refused = admit(&ManagerSpawnFacts {
        live: true,
        ..facts()
    });
    assert_eq!(code(refused), TEAM_ROLE_ALREADY_LIVE);
}

#[test]
fn a_custom_manager_is_never_task_bound_and_must_be_registered() {
    assert_eq!(
        code(admit(&ManagerSpawnFacts {
            task_bound: true,
            ..facts()
        })),
        TEAM_ROLE_NOT_TASK_BOUND
    );
    assert_eq!(
        code(admit(&ManagerSpawnFacts {
            registered: false,
            ..facts()
        })),
        codeterm_types::team::staffing::TEAM_MEMBER_ROLE_UNKNOWN
    );
}

#[test]
fn a_reservation_holds_its_name_until_dropped() {
    let key = ("team_reserve".to_string(), "fixture_manager".to_string());
    let reservation = ManagerReservation { key: key.clone() };
    reserved().lock().unwrap().insert(key.clone());
    assert!(reserved().lock().unwrap().contains(&key));
    drop(reservation);
    assert!(!reserved().lock().unwrap().contains(&key));
}
