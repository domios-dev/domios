use super::*;
use std::sync::Arc;

fn state() -> Arc<parking_lot::Mutex<UpdateState>> {
    Arc::new(parking_lot::Mutex::new(UpdateState {
        install_method: InstallMethod::Unknown,
        last_check: None,
        last_check_time: None,
        channel: Channel::Stable,
        phase: UpdatePhase::Idle,
        last_error: Some("stale error".to_string()),
        phase_changed_at: None,
        downloaded_bytes: 0,
        download_total_bytes: None,
    }))
}

#[test]
fn update_apply_phase_begins_checking_and_clears_error() {
    let update_state = state();

    begin_apply(&update_state);

    let state = update_state.lock();
    assert_eq!(state.phase, UpdatePhase::Checking);
    assert!(state.last_error.is_none());
}
