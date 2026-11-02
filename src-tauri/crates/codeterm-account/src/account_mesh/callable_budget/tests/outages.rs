use super::*;
use crate::account_mesh::{callable::CallableFailure, wire::*};
use std::sync::atomic::{AtomicBool, Ordering};

struct OutageClock {
    time: ClockPort,
    maximum_jitter: bool,
}

#[async_trait::async_trait]
impl Clock for OutageClock {
    fn now(&self) -> Instant {
        self.time.now()
    }

    async fn wait(&self, delay: Duration) {
        self.time.wait(delay).await;
    }

    fn jitter_seconds(&self, upper_bound: u64) -> u64 {
        if self.maximum_jitter {
            upper_bound
        } else {
            0
        }
    }
}

struct OutageTransport<'a> {
    clock: &'a OutageClock,
    route: &'a str,
    status: u16,
    retry_after_seconds: Option<u64>,
    failing: AtomicBool,
    calls: parking_lot::Mutex<Vec<(String, Instant)>>,
}

#[async_trait::async_trait]
impl crate::account_mesh::callable::CallableTransport for OutageTransport<'_> {
    async fn call(
        &self,
        route: &str,
        _: &str,
        _: serde_json::Value,
    ) -> Result<serde_json::Value, CallableFailure> {
        self.calls.lock().push((route.into(), self.clock.now()));
        if route == self.route && self.failing.load(Ordering::SeqCst) {
            Err(CallableFailure::Status {
                status: self.status,
                body: None,
                retry_after_seconds: self.retry_after_seconds,
            })
        } else {
            Ok(serde_json::json!({"result": true}))
        }
    }
}

#[tokio::test]
async fn mesh_5xx_delays_every_peer_with_bounded_exponential_jitter() {
    for route in [ROUTE_ADMISSION, ROUTE_CHALLENGE] {
        for status in [500, 503, 599] {
            for maximum_jitter in [false, true] {
                let uid = format!("mesh-outage-{route}-{status}-{maximum_jitter}");
                let clock = OutageClock {
                    time: ClockPort::new(),
                    maximum_jitter,
                };
                let transport = OutageTransport {
                    clock: &clock,
                    route,
                    status,
                    retry_after_seconds: None,
                    failing: true.into(),
                    calls: Default::default(),
                };
                let guard = guard();
                let other = if route == ROUTE_ADMISSION {
                    ROUTE_CHALLENGE
                } else {
                    ROUTE_ADMISSION
                };
                let delays = if maximum_jitter {
                    [60, 120, 240, 300, 300, 300, 300]
                } else {
                    [30, 60, 120, 150, 150, 150, 150]
                };
                for seconds in delays {
                    let before = clock.now();
                    assert!(run(
                        &transport,
                        &uid,
                        route,
                        "",
                        serde_json::Value::Null,
                        &guard,
                        &clock
                    )
                    .await
                    .is_err());
                    assert_eq!(transport.calls.lock().len() % 2, 1);
                    assert!(run(
                        &transport,
                        &uid,
                        other,
                        "",
                        serde_json::Value::Null,
                        &guard,
                        &clock
                    )
                    .await
                    .is_ok());
                    assert_eq!(
                        transport.calls.lock().last().unwrap().1 - before,
                        Duration::from_secs(seconds)
                    );
                }
                assert_eq!(*clock.time.waits.lock(), delays.map(Duration::from_secs));
                assert_eq!(transport.calls.lock().len(), 14);
            }
        }
    }
}

#[tokio::test]
async fn mesh_5xx_preserves_retry_after_and_only_same_route_success_resets_backoff() {
    let uid = "mesh-outage-recovery";
    let clock = OutageClock {
        time: ClockPort::new(),
        maximum_jitter: false,
    };
    let mut transport = OutageTransport {
        clock: &clock,
        route: ROUTE_ADMISSION,
        status: 503,
        retry_after_seconds: Some(700),
        failing: true.into(),
        calls: Default::default(),
    };
    let guard = guard();
    assert!(run(
        &transport,
        uid,
        ROUTE_ADMISSION,
        "",
        serde_json::Value::Null,
        &guard,
        &clock
    )
    .await
    .is_err());
    transport.failing.store(false, Ordering::SeqCst);
    assert!(run(
        &transport,
        uid,
        ROUTE_ADMISSION,
        "",
        serde_json::Value::Null,
        &guard,
        &clock
    )
    .await
    .is_ok());
    transport.failing.store(true, Ordering::SeqCst);
    transport.retry_after_seconds = None;
    assert!(run(
        &transport,
        uid,
        ROUTE_ADMISSION,
        "",
        serde_json::Value::Null,
        &guard,
        &clock
    )
    .await
    .is_err());
    assert!(run(
        &transport,
        uid,
        ROUTE_CHALLENGE,
        "",
        serde_json::Value::Null,
        &guard,
        &clock
    )
    .await
    .is_ok());
    assert_eq!(*clock.time.waits.lock(), [700, 30].map(Duration::from_secs));
    assert_eq!(transport.calls.lock().len(), 4);
}
