use super::*;

#[test]
fn burst_of_state_changes_for_one_tab_runs_at_most_two_replays() {
    let coalescer = PlannerReplayCoalescer::default();
    let owners = (0..500).filter(|_| coalescer.request("tab")).count();
    assert_eq!(owners, 1);

    let mut runs = 0;
    coalescer.drain("tab", || {
        runs += 1;
        if runs == 1 {
            for _ in 0..500 {
                assert!(!coalescer.request("tab"));
            }
        }
    });

    assert_eq!(runs, 2);
    assert!(
        coalescer.request("tab"),
        "a drained tab accepts a new owner"
    );
}

#[test]
fn idle_request_runs_once_and_tabs_are_independent() {
    let coalescer = PlannerReplayCoalescer::default();
    assert!(coalescer.request("a"));
    assert!(coalescer.request("b"));
    let mut runs = 0;
    coalescer.drain("a", || runs += 1);
    assert_eq!(runs, 1);
    assert!(!coalescer.request("b"));
    assert!(coalescer.finish("b"));
    assert!(!coalescer.finish("b"));
}

#[test]
fn panicking_replay_releases_the_tab() {
    let coalescer = PlannerReplayCoalescer::default();
    assert!(coalescer.request("tab"));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        coalescer.drain("tab", || panic!("replay failed"));
    }));
    assert!(outcome.is_err());
    assert!(coalescer.request("tab"));
}
