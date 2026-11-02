use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn general_and_restored_launches_wait_for_provider_writes() {
    let (release, released) = mpsc::channel();
    let (events, observed) = mpsc::channel();
    let cli_events = events.clone();
    let cli = std::thread::spawn(move || {
        released.recv().unwrap();
        cli_events.send("cli-written").unwrap();
    });
    let (hooks_release, hooks_released) = mpsc::channel();
    let startup = std::thread::spawn(move || {
        let complete = complete(
            Some(cli),
            [|| {
                hooks_released.recv().unwrap();
                events.send("hooks-written").unwrap();
            }],
            || {
                events.send("rules-written").unwrap();
            },
        )
        .unwrap();
        let _complete = complete;
        events.send("general-spawn").unwrap();
        events.send("restore-spawn").unwrap();
    });
    assert!(observed.try_recv().is_err());
    release.send(()).unwrap();
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(5)).unwrap(),
        "cli-written"
    );
    assert!(observed.try_recv().is_err());
    hooks_release.send(()).unwrap();
    for expected in [
        "hooks-written",
        "rules-written",
        "general-spawn",
        "restore-spawn",
    ] {
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            expected
        );
    }
    startup.join().unwrap();
}

#[test]
fn failed_provider_writer_never_authorizes_launch() {
    let cli = std::thread::spawn(|| panic!("opaque-install-failure"));
    let result = complete(Some(cli), std::iter::empty::<fn()>(), || {
        panic!("must not install rules")
    });
    assert!(result.is_err());
}
