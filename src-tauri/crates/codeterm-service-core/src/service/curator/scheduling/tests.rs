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
