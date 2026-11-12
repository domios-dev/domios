use super::*;

#[test]
fn unbound_or_absent_owner_has_no_timed_wake_and_binding_resumes_once() {
    for failure in [
        CuratorFailure::OrchestratorUnbound,
        CuratorFailure::OrchestratorAbsent,
    ] {
        let clock = crate::cron::clock::MockClock::starting_at(1_000);
        let mut state = ControlLoop::default();
        state.next_at = Some(1_001);
        reconcile_idle_loop(&mut state, Some(failure), Some(900), 1_000);
        let waiting = state.clone();
        assert_eq!(waiting.next_at, None);
        for delta in [1_000, 60_000, 86_400_000] {
            let now = clock.advance_ms(delta);
            reconcile_idle_loop(&mut state, Some(failure), Some(900), now);
            assert_eq!(state, waiting);
        }
        let now = clock.advance_ms(1);
        reconcile_idle_loop(&mut state, None, Some(900), now);
        assert_eq!(state.fallback_reason, None);
        assert_eq!(state.next_at, Some(now + 1_000));
        let resumed = state.clone();
        reconcile_idle_loop(&mut state, None, Some(900), now + 1);
        assert_eq!(state, resumed);
    }
}

#[test]
fn a_curator_round_runs_on_its_registered_pick_and_otherwise_mirrors_the_orchestrator() {
    use codeterm_team_package::{ModelPreference, RoleStart, StartMode, TeamRoleGroup};
    use codeterm_types::team::role_registration::{PinnedPackage, RoleRegistration};

    let mirror = Mirror {
        provider: "orchestrator-harness".into(),
        launch_profile: Some("orchestrator-profile".into()),
        model: "orchestrator-model".into(),
        effort: Some("orchestrator-effort".into()),
    };
    let registration = |model: Option<ModelPreference>| RoleRegistration {
        team_id: "team_probe".into(),
        group: TeamRoleGroup::Manager,
        name: "curator".into(),
        label: "curator".into(),
        description: None,
        start: RoleStart::Mode(StartMode::OnDemand),
        package: PinnedPackage {
            name: "fixture/package".into(),
            version: "1.0.0".into(),
        },
        instructions: "fixture-prose".into(),
        model,
        reasoning: Some("picked-effort".into()),
        placement_peer: None,
        consent_decision_id: None,
        files: Vec::new(),
        skills: Vec::new(),
        registered_at: 1,
    };

    let picked = registration(Some(ModelPreference::List(vec![
        "picked-harness:picked-model".into(),
    ])));
    assert_eq!(
        curator_seat(mirror.clone(), Some(&picked)),
        Mirror {
            provider: "picked-harness".into(),
            launch_profile: None,
            model: "picked-model".into(),
            effort: Some("picked-effort".into()),
        }
    );
    assert_eq!(curator_seat(mirror.clone(), None), mirror);
    assert_eq!(
        curator_seat(mirror.clone(), Some(&registration(None))),
        mirror,
        "a registration without a model keeps the Orchestrator mirror"
    );
}
