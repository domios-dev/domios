//! Permission is established when a surface is enabled, and nothing claims a
//! delivery it did not get: the gate branches on the OS answer, and the send's
//! own Result decides whether the toast or the fallback window carried it.
use super::*;
use std::cell::Cell;

#[test]
fn notify_permission_a_failed_send_is_not_reported_as_delivered() {
    let route = route_toast(true, || {
        Err::<(), &str>("the toast never reached the shell")
    });
    assert_eq!(
        route,
        ToastRoute::FallbackWindow,
        "a send that failed is not a delivery, so its Result must be consulted"
    );

    let route = route_toast(true, || Ok::<(), &str>(()));
    assert_eq!(
        route,
        ToastRoute::Native,
        "and a send that succeeded is the one case that may claim the toast"
    );
}

#[test]
fn notify_permission_enabling_asks_the_os_and_disabling_asks_nothing() {
    let asked = Cell::new(0);

    let answer = establish_with(true, || {
        asked.set(asked.get() + 1);
        PermissionState::Denied
    });
    assert_eq!(asked.get(), 1, "switching a surface ON asks the OS");
    assert_eq!(
        answer,
        Some(PermissionState::Denied),
        "and the gesture carries back the OS answer, not the stored config"
    );

    let answer = establish_with(false, || {
        asked.set(asked.get() + 1);
        PermissionState::Granted
    });
    assert_eq!(
        asked.get(),
        1,
        "switching a surface OFF asks nothing: withdrawing needs no consent"
    );
    assert_eq!(answer, None, "so it has no fresh answer to report");
}

#[test]
fn notify_permission_gate_refuses_until_the_os_grants() {
    assert!(
        !native_toast_can_deliver(),
        "nothing has asked the OS in this process, so the gate may not claim delivery"
    );
    assert!(
        !toast_can_deliver(PermissionState::Denied, true),
        "a refusal is not deliverable on any platform"
    );
    assert!(
        !toast_can_deliver(PermissionState::Prompt, true),
        "and an unasked permission is not a grant"
    );
    assert!(
        toast_can_deliver(PermissionState::Granted, true),
        "a grant on an open channel is the only deliverable combination"
    );
    assert!(
        !toast_can_deliver(PermissionState::Granted, false),
        "the Windows AppUserModelID condition keeps its veto over a grant"
    );
}

/// Regression for ct#324: the plugin's desktop `permission_state`/
/// `request_permission` are stubs that always answer `Granted`. The real
/// Windows answer must come from the shell's own toast switch, not that stub.
#[test]
fn notify_permission_branches_on_the_real_switch_not_the_plugin_stub() {
    assert_eq!(
        permission_from_toast_switch(Some(0)),
        PermissionState::Denied,
        "the plugin's stub always answers Granted; the real switch must override it"
    );
    assert_eq!(
        permission_from_toast_switch(Some(1)),
        PermissionState::Granted
    );
    assert_eq!(
        permission_from_toast_switch(None),
        PermissionState::Granted,
        "an absent registry entry is the factory default, which presents toasts"
    );
}

#[test]
fn notify_permission_where_delivery_cannot_happen_the_fallback_window_is_reachable() {
    let attempted = Cell::new(false);
    let route = route_toast(false, || {
        attempted.set(true);
        Ok::<(), &str>(())
    });
    assert_eq!(
        route,
        ToastRoute::FallbackWindow,
        "an undeliverable toast selects the existing fallback window"
    );
    assert!(
        !attempted.get(),
        "and the gate stops the send instead of declaring delivery fine and dropping it"
    );
}
