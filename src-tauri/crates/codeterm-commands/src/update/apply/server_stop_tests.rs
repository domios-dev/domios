use super::server_stop_owes_relaunch;
use crate::application_host::ServerStopError;

#[test]
fn confirmed_server_stop_owes_a_relaunch_after_a_failed_install() {
    assert!(server_stop_owes_relaunch(&Ok(())));
}

#[test]
fn refusal_before_dispatch_keeps_the_running_pair_without_relaunch() {
    let refused = Err(ServerStopError {
        dispatched: false,
        reason: "opaque-refusal".into(),
    });
    assert!(!server_stop_owes_relaunch(&refused));
}

#[test]
fn unconfirmed_exit_after_dispatch_still_owes_a_relaunch() {
    let uncertain = Err(ServerStopError {
        dispatched: true,
        reason: "opaque-uncertainty".into(),
    });
    assert!(server_stop_owes_relaunch(&uncertain));
}
