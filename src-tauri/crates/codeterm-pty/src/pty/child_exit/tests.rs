use super::ChildExit;
use std::cell::Cell;

#[test]
fn failed_native_wait_does_not_release_the_pty() {
    let exit = ChildExit::default();
    let released = Cell::new(false);

    exit.observed(Err("native wait failed".into()), || released.set(true));

    assert!(
        !released.get(),
        "failed observation cannot authorize PTY closure"
    );
    assert_eq!(exit.resolved(), Some(None));
    assert_eq!(exit.code(), None);
}

#[test]
fn observed_exit_records_the_code_before_releasing_once() {
    let exit = ChildExit::default();
    let releases = Cell::new(0);

    exit.observed(Ok(Some(3)), || {
        assert_eq!(exit.resolved(), Some(Some(3)));
        releases.set(releases.get() + 1);
    });
    exit.observed(Ok(Some(7)), || releases.set(releases.get() + 1));

    assert_eq!(releases.get(), 1);
    assert_eq!(exit.code(), Some(3));
}

#[test]
fn observed_exit_without_a_code_still_releases_the_pty() {
    let exit = ChildExit::default();
    let released = Cell::new(false);

    exit.observed(Ok(None), || released.set(true));

    assert!(
        released.get(),
        "a signaled process authorizes PTY closure even if its code query failed"
    );
    assert_eq!(exit.resolved(), Some(None));
}
