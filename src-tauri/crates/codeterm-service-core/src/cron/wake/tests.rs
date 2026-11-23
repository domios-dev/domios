use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};

use super::CronWakeSignal;

const FAILURE_BOUND: Duration = Duration::from_secs(30);

#[test]
fn notification_raised_during_a_tick_ends_the_next_wait_immediately() {
    let wake = CronWakeSignal::default();
    let observed = wake.observe();
    wake.notify_all();
    let started = Instant::now();
    assert!(wake.wait_after(observed, FAILURE_BOUND));
    assert!(started.elapsed() < FAILURE_BOUND);
    assert!(!wake.wait_after(wake.observe(), Duration::ZERO));
}

#[test]
fn waiting_scheduler_wakes_on_a_notification_from_another_thread() {
    let wake = Arc::new(CronWakeSignal::default());
    let observed = wake.observe();
    let (parked_tx, parked) = mpsc::channel();
    let waiter = std::thread::spawn({
        let wake = wake.clone();
        move || {
            parked_tx.send(()).unwrap();
            wake.wait_after(observed, FAILURE_BOUND)
        }
    });
    parked.recv().unwrap();
    wake.notify_all();
    assert!(waiter.join().unwrap());
}

#[test]
fn scheduler_loop_under_notification_storm_never_loses_the_last_change() {
    const NOTIFIERS: usize = 8;
    const NOTIFICATIONS: usize = 500;
    let wake = Arc::new(CronWakeSignal::default());
    let changes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(NOTIFIERS + 1));
    let (done_tx, done) = mpsc::channel();
    let scheduler = std::thread::spawn({
        let (wake, changes, start) = (wake.clone(), changes.clone(), start.clone());
        move || {
            start.wait();
            let target = NOTIFIERS * NOTIFICATIONS;
            let mut ticks = 0usize;
            loop {
                let observed = wake.observe();
                ticks += 1;
                if changes.load(std::sync::atomic::Ordering::Acquire) == target {
                    break;
                }
                assert!(
                    wake.wait_after(observed, FAILURE_BOUND),
                    "a committed change was not delivered to the scheduler"
                );
            }
            done_tx.send(ticks).unwrap();
        }
    });
    let notifiers: Vec<_> = (0..NOTIFIERS)
        .map(|_| {
            let (wake, changes, start) = (wake.clone(), changes.clone(), start.clone());
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..NOTIFICATIONS {
                    changes.fetch_add(1, std::sync::atomic::Ordering::Release);
                    wake.notify_all();
                }
            })
        })
        .collect();
    for notifier in notifiers {
        notifier.join().unwrap();
    }
    let ticks = done.recv_timeout(FAILURE_BOUND).expect("scheduler stalled");
    assert!(ticks >= 1);
    scheduler.join().unwrap();
}
