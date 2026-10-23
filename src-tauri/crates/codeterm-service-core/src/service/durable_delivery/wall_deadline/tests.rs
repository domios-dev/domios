use super::WallDeadline;
use crate::cron::clock::{Clock, SystemClock};
use std::sync::mpsc;
use std::time::Duration;

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod platform;

#[test]
fn blueprint_v2_call_durable_review_native_wall_deadline_fires_once_at_an_expired_absolute_time() {
    let (tx, rx) = mpsc::channel();
    let _timer = WallDeadline::arm(SystemClock.now_ms().saturating_sub(1), move || {
        let _ = tx.send(());
    })
    .unwrap();
    rx.recv_timeout(Duration::from_secs(5))
        .expect("native absolute deadline fires without polling");
    assert!(rx.try_recv().is_err());
}
