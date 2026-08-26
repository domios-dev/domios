//! A toast the shell discards must not be reported as delivered. The desktop
//! send answers `Ok` once it has handed the toast off, whatever the shell then
//! does with it, so the route is decided by reading the shell instead.
use super::*;
use std::path::PathBuf;

fn dir(parts: &[&str]) -> PathBuf {
    parts.iter().collect()
}

fn installed_dir() -> PathBuf {
    dir(&["C:", "Program Files", "CodeTerm"])
}

fn target_dir(profile: &str) -> PathBuf {
    dir(&["D:", "Developer", "codeterm", "target", profile])
}

/// Stands in for the desktop send: it reports hand-off and cannot report
/// arrival, which is why its `Ok` may not decide the route on its own.
fn a_send_that_cannot_fail() -> Result<(), &'static str> {
    Ok(())
}

#[test]
fn toast_delivery_a_toast_the_shell_discards_is_not_reported_as_delivered() {
    assert!(
        !shell_presents_toasts(Some(0)),
        "the master switch off means the shell takes every toast and presents none"
    );

    let deliverable = toast_can_deliver(
        PermissionState::Granted,
        windows_toast_reaches_the_shell(Some(&installed_dir())) && shell_presents_toasts(Some(0)),
    );
    assert!(
        !deliverable,
        "a granted permission on an addressed toast is not delivery when the shell discards it"
    );
    assert_eq!(
        route_toast(deliverable, a_send_that_cannot_fail),
        ToastRoute::FallbackWindow,
        "so the fallback window carries it rather than an unfailable Ok claiming success"
    );
}

#[test]
fn toast_delivery_a_toast_the_shell_presents_is_still_reported_as_delivered() {
    assert!(
        shell_presents_toasts(None),
        "an unwritten switch is the factory state, which is on: silence is not a veto"
    );
    assert!(shell_presents_toasts(Some(1)), "and an explicit on is on");

    let deliverable = toast_can_deliver(
        PermissionState::Granted,
        windows_toast_reaches_the_shell(Some(&installed_dir())) && shell_presents_toasts(Some(1)),
    );
    assert_eq!(
        route_toast(deliverable, a_send_that_cannot_fail),
        ToastRoute::Native,
        "reading the shell must not make every send look failed and open the window each time"
    );
}

#[test]
fn toast_delivery_the_app_user_model_id_condition_still_refuses_a_target_build() {
    for profile in ["debug", "release"] {
        let exe_dir = target_dir(profile);
        assert!(
            !windows_toast_reaches_the_shell(Some(&exe_dir)),
            "a binary under target/{profile} carries no AppUserModelID"
        );

        let deliverable = toast_can_deliver(
            PermissionState::Granted,
            windows_toast_reaches_the_shell(Some(&exe_dir)) && shell_presents_toasts(None),
        );
        assert_eq!(
            route_toast(deliverable, a_send_that_cannot_fail),
            ToastRoute::FallbackWindow,
            "and an enabled shell does not rescue a toast that carries no address"
        );
    }
}
