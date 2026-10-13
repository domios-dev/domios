use super::*;
use crate::service::agent_runtime::model::RuntimeState;

#[test]
fn teardown_preserves_degraded_reason_and_refuses_old_epoch() {
    let mut state = RuntimeState::unloaded(Some("own".into()));
    let epoch = state.begin();
    state.load(epoch, "own");
    let failure = RuntimeFailure::HookFailed("durable status write failed".into());
    state.degrade(epoch, &failure);
    let reason = state.snapshot().reason;
    assert!(!unload_retaining_degradation(
        &mut state,
        epoch + 1,
        &RuntimeFailure::PipeEof
    ));
    assert!(unload_retaining_degradation(
        &mut state,
        epoch,
        &RuntimeFailure::PipeEof
    ));
    assert!(matches!(state.phase, RuntimePhase::Degraded { .. }));
    assert_eq!(state.snapshot().reason, reason);
}

#[test]
fn ordinary_pipe_exit_still_unloads_without_fabricating_a_degraded_state() {
    let mut state = RuntimeState::unloaded(Some("own".into()));
    let epoch = state.begin();
    state.load(epoch, "own");
    assert!(unload_retaining_degradation(
        &mut state,
        epoch,
        &RuntimeFailure::PipeEof
    ));
    assert!(matches!(state.phase, RuntimePhase::Unloaded { .. }));
    assert!(!unload_retaining_degradation(
        &mut state,
        epoch,
        &RuntimeFailure::PipeEof
    ));
}
