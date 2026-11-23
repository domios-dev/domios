use super::*;
use crate::lock_hold::{TimedMutex, TimedRwLock};

fn on_thread(name: &str, work: impl FnOnce() + Send + 'static) -> Vec<String> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(work)
        .unwrap()
        .join()
        .unwrap();
    violations_mentioning(&format!("thread={name}"))
}

#[test]
fn canonical_order_and_try_acquisitions_are_clean() {
    let reports = on_thread("order-canonical", || {
        let mux = TimedRwLock::new("mux", ());
        let output = TimedMutex::new("output_buf", ());
        let parser = TimedMutex::new("screen_state_parser", ());
        let tracker = TimedMutex::new("agent_tracker", ());
        let pty = TimedMutex::new("pty", ());
        let _mux = mux.read();
        let _output = output.lock();
        let _parser = parser.lock();
        let _tracker = tracker.lock();
        let _pty = pty.lock();
        assert!(mux.try_write().is_none());
        assert_eq!(
            held_names(),
            [
                "mux",
                "output_buf",
                "screen_state_parser",
                "agent_tracker",
                "pty"
            ]
        );
    });
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn inversion_is_reported_with_both_sites() {
    let reports = on_thread("order-inversion", || {
        let mux = TimedRwLock::new("mux", ());
        let tracker = TimedMutex::new("agent_tracker", ());
        let _tracker = tracker.lock();
        let _mux = mux.read();
    });
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].contains("mux acquired at"));
    assert!(reports[0].contains("while holding agent_tracker"));
    assert!(reports[0].contains(file!()));
}

#[test]
fn recursive_shared_read_is_reported_without_blocking() {
    let reports = on_thread("order-recursive-read", || {
        let mux = TimedRwLock::new("mux", ());
        let _outer = mux.read();
        let _inner = mux.read();
    });
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].contains("recursive shared acquisition of mux"));
}

#[test]
fn same_rank_instances_nest_as_violation_and_unranked_locks_do_not() {
    let reports = on_thread("order-same-rank", || {
        let first = TimedMutex::new("screen_state_parser", ());
        let second = TimedMutex::new("screen_state_parser", ());
        let local = TimedMutex::new("launch_registry", ());
        let _local = local.lock();
        let _first = first.lock();
        let _second = second.lock();
    });
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].contains("while holding screen_state_parser"));
}

#[test]
fn released_guards_leave_the_held_set_in_any_drop_order() {
    let reports = on_thread("order-release", || {
        let mux = TimedRwLock::new("mux", ());
        let pty = TimedMutex::new("pty", ());
        let read = mux.read();
        let held = pty.lock();
        drop(read);
        drop(held);
        assert!(held_names().is_empty());
        drop(pty.lock());
        let _mux = mux.write();
    });
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn blocking_wait_under_a_guard_is_reported() {
    let reports = on_thread("order-blocking-wait", || {
        let pty = TimedMutex::new("pty", ());
        check_blocking_wait("block_on");
        let _pty = pty.lock();
        check_blocking_wait("block_on");
    });
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].contains("block_on at"));
    assert!(reports[0].contains("while holding pty"));
}

#[test]
fn concurrent_canonical_load_never_reports_or_deadlocks() {
    use std::sync::{Arc, Barrier};
    let locks = Arc::new((
        TimedRwLock::new("mux", 0u64),
        TimedMutex::new("output_buf", 0u64),
        TimedMutex::new("agent_tracker", 0u64),
        TimedMutex::new("pty", 0u64),
    ));
    const WORKERS: usize = 4;
    const ROUNDS: u64 = 500;
    let start = Arc::new(Barrier::new(WORKERS));
    let workers: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let (locks, start) = (locks.clone(), start.clone());
            std::thread::Builder::new()
                .name(format!("order-load-{worker}"))
                .spawn(move || {
                    start.wait();
                    for round in 0..ROUNDS {
                        if (round + worker as u64) % 4 == 0 {
                            *locks.0.write() += 1;
                        }
                        let _mux = locks.0.read();
                        *locks.1.lock() += 1;
                        let mut tracker = locks.2.lock();
                        *tracker += 1;
                        *locks.3.lock() += 1;
                    }
                })
                .unwrap()
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(*locks.1.lock(), WORKERS as u64 * ROUNDS);
    assert_eq!(*locks.3.lock(), WORKERS as u64 * ROUNDS);
    assert!(violations_mentioning("thread=order-load-").is_empty());
}
