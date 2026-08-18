use std::cell::RefCell;

use super::*;

#[derive(Debug, PartialEq, Eq)]
enum Half {
    Declare,
    Persist,
}

fn halves_run(is_macos: bool, shutting_down: bool) -> Vec<Half> {
    let calls = RefCell::new(Vec::new());
    execute_exit_plan(
        plan_run_exit(is_macos, shutting_down),
        || calls.borrow_mut().push(Half::Declare),
        || calls.borrow_mut().push(Half::Persist),
    );
    calls.into_inner()
}

#[test]
fn macos_unrequested_exit_persists_without_declaring_clean_exit() {
    assert_eq!(halves_run(true, false), vec![Half::Persist]);
}

#[test]
fn macos_genuine_quit_declares_clean_exit_before_persisting() {
    assert_eq!(halves_run(true, true), vec![Half::Declare, Half::Persist]);
}

#[test]
fn non_macos_exit_runs_both_halves_regardless_of_quit_request() {
    assert_eq!(halves_run(false, false), vec![Half::Declare, Half::Persist]);
    assert_eq!(halves_run(false, true), vec![Half::Declare, Half::Persist]);
}

#[test]
fn every_exit_route_persists_state() {
    for is_macos in [true, false] {
        for shutting_down in [true, false] {
            assert!(plan_run_exit(is_macos, shutting_down).persist_state);
        }
    }
}

#[test]
fn only_a_genuine_quit_declares_clean_exit_on_macos() {
    assert!(!plan_run_exit(true, false).declare_clean_exit);
    assert!(plan_run_exit(true, true).declare_clean_exit);
}
