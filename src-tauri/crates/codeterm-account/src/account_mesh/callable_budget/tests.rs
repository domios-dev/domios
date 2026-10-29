use super::*;

#[test]
fn account_wide_budget_uses_the_longer_deadline_and_serializes_the_probe() {
    let neighbors = account("budget-fixture");
    let mesh = account("budget-fixture");
    assert!(Arc::ptr_eq(&neighbors, &mesh));
    assert!(!Arc::ptr_eq(&neighbors, &account("another-budget-fixture")));
    let start = Instant::now();
    let mut gate = neighbors.try_lock().unwrap();
    gate.limited("admission", start, 45);
    assert_eq!(gate.remaining(start), Duration::from_secs(45));
    assert!(mesh.try_lock().is_err());
    drop(gate);
    let mut gate = mesh.try_lock().unwrap();
    assert_eq!(
        gate.remaining(start + Duration::from_secs(44)),
        Duration::from_secs(1)
    );
    gate.limited("admission", start + Duration::from_secs(45), 0);
    assert_eq!(
        gate.remaining(start + Duration::from_secs(45)),
        Duration::from_secs(2)
    );
    for _ in 0..400 {
        gate.limited("admission", start + Duration::from_secs(45), 0);
    }
    assert_eq!(
        gate.remaining(start + Duration::from_secs(45)),
        Duration::from_secs(300)
    );
    gate.success("admission");
    assert_eq!(
        gate.remaining(start + Duration::from_secs(345)),
        Duration::ZERO
    );
}

#[test]
fn retry_after_parses_seconds_dates_and_rounds_up_fractional_deadlines() {
    let epoch = SystemTime::UNIX_EPOCH;
    let date = epoch + Duration::from_secs(1445412480);
    assert_eq!(retry_after("45", epoch), Some(45));
    assert_eq!(
        retry_after(
            "Wed, 21 Oct 2015 07:28:00 GMT",
            date - Duration::from_millis(1500)
        ),
        Some(2)
    );
    assert_eq!(
        retry_after(
            "Wed, 21 Oct 2015 07:28:00 GMT",
            date + Duration::from_secs(1)
        ),
        Some(0)
    );
    for value in ["invalid", "-1", "", "Wed, 99 Oct 2015 07:28:00 GMT"] {
        assert_eq!(retry_after(value, epoch), None);
    }
}

struct ClockPort {
    start: Instant,
    elapsed: std::sync::atomic::AtomicU64,
    waits: parking_lot::Mutex<Vec<Duration>>,
}
impl ClockPort {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            elapsed: 0.into(),
            waits: Default::default(),
        }
    }
}
#[async_trait::async_trait]
impl Clock for ClockPort {
    fn now(&self) -> Instant {
        self.start + Duration::from_millis(self.elapsed.load(std::sync::atomic::Ordering::SeqCst))
    }
    async fn wait(&self, delay: Duration) {
        self.waits.lock().push(delay);
        self.elapsed.fetch_add(
            delay.as_millis() as u64,
            std::sync::atomic::Ordering::SeqCst,
        );
    }
}

struct TransportPort(parking_lot::Mutex<Vec<String>>);
#[async_trait::async_trait]
impl super::super::callable::CallableTransport for TransportPort {
    async fn call(
        &self,
        route: &str,
        _: &str,
        _: serde_json::Value,
    ) -> Result<serde_json::Value, super::super::callable::CallableFailure> {
        self.0.lock().push(route.into());
        if route == "admission" {
            Err(super::super::callable::CallableFailure::Status {
                status: 429,
                body: None,
                retry_after_seconds: Some(3),
            })
        } else {
            Ok(serde_json::json!({"result":true}))
        }
    }
}

struct GuardPort {
    authority: tokio::sync::watch::Sender<Option<(String, u64)>>,
    expected: (String, u64),
}
#[async_trait::async_trait]
impl super::super::callable::CallableGuard for GuardPort {
    async fn current(&self) -> bool {
        self.authority.borrow().as_ref() == Some(&self.expected)
    }
    async fn retired(&self) {
        let mut changes = self.authority.subscribe();
        loop {
            if changes.borrow_and_update().as_ref() != Some(&self.expected) {
                return;
            }
            if changes.changed().await.is_err() {
                return;
            }
        }
    }
}
fn guard() -> GuardPort {
    GuardPort {
        authority: tokio::sync::watch::channel(Some(("owner".into(), 1))).0,
        expected: ("owner".into(), 1),
    }
}

#[tokio::test]
async fn successful_challenges_and_other_routes_do_not_erase_admission_throttle_history() {
    let transport = TransportPort(Default::default());
    let clock = ClockPort::new();
    let guard = guard();
    let uid = "alternating-admission-fixture";
    for _ in 0..10 {
        let (challenge, other) = tokio::join!(
            run(
                &transport,
                uid,
                "challenge",
                "",
                serde_json::Value::Null,
                &guard,
                &clock
            ),
            run(
                &transport,
                uid,
                "other",
                "",
                serde_json::Value::Null,
                &guard,
                &clock
            ),
        );
        assert!(challenge.is_ok());
        assert!(other.is_ok());
        assert!(run(
            &transport,
            uid,
            "admission",
            "",
            serde_json::Value::Null,
            &guard,
            &clock
        )
        .await
        .is_err());
    }
    assert_eq!(
        *clock.waits.lock(),
        [3, 3, 4, 8, 16, 32, 64, 128, 256].map(Duration::from_secs)
    );
    assert_eq!(transport.0.lock().len(), 30);
    let budget = account(uid);
    assert_eq!(
        budget.lock().await.remaining(clock.now()),
        Duration::from_secs(300)
    );
}

struct ParkedClock {
    time: ClockPort,
    waiting: tokio::sync::mpsc::UnboundedSender<()>,
    release: tokio::sync::Semaphore,
}
#[async_trait::async_trait]
impl Clock for ParkedClock {
    fn now(&self) -> Instant {
        self.time.now()
    }
    async fn wait(&self, delay: Duration) {
        self.waiting.send(()).unwrap();
        self.release.acquire().await.unwrap().forget();
        self.time.wait(delay).await;
    }
}

#[tokio::test]
async fn parked_mutations_retire_on_signout_or_generation_change_without_transport_dispatch() {
    use super::super::callable::CallableFailure;
    for route in ["redeemGiftKey", "friendsSetFriendReceiver"] {
        for replacement in [None, Some(("other".into(), 1)), Some(("owner".into(), 2))] {
            let uid = format!("parked-{route}-{replacement:?}");
            let transport = TransportPort(Default::default());
            let (waiting, mut observed) = tokio::sync::mpsc::unbounded_channel();
            let clock = ParkedClock {
                time: ClockPort::new(),
                waiting,
                release: tokio::sync::Semaphore::new(0),
            };
            let waiting_guard = guard();
            account(&uid).lock().await.limited(route, clock.now(), 45);
            let request = run(
                &transport,
                &uid,
                route,
                "",
                serde_json::Value::Null,
                &waiting_guard,
                &clock,
            );
            let retire = async {
                observed.recv().await.unwrap();
                waiting_guard.authority.send_replace(replacement);
            };
            let (result, _) = tokio::join!(request, retire);
            assert_eq!(result, Err(CallableFailure::Retired));
            assert!(transport.0.lock().is_empty());
            let live = guard();
            let resume = async {
                observed.recv().await.unwrap();
                clock.release.add_permits(1);
            };
            let (result, _) = tokio::join!(
                run(
                    &transport,
                    &uid,
                    route,
                    "",
                    serde_json::Value::Null,
                    &live,
                    &clock
                ),
                resume,
            );
            assert!(result.is_ok());
            assert_eq!(*transport.0.lock(), [route]);
            assert_eq!(*clock.time.waits.lock(), [Duration::from_secs(45)]);
        }
    }
}

#[test]
fn captured_cleanup_challenges_allow_only_closing_the_captured_installation() {
    use super::super::{
        callable::cleanup_challenge,
        wire::{ChallengeRequest, ProofEndpoint},
    };
    for (body, allowed) in [
        (serde_json::json!({"operation":"withdraw"}), true),
        (
            serde_json::json!({"operation":"presence","presence":"signed_out"}),
            true,
        ),
        (
            serde_json::json!({"operation":"presence","presence":"present"}),
            false,
        ),
        (
            serde_json::json!({"operation":"session_revoke","installation_id":"self","target":{"installation_id":"self"}}),
            true,
        ),
        (
            serde_json::json!({"operation":"session_revoke","installation_id":"self","target":{"installation_id":"other"}}),
            false,
        ),
        (serde_json::json!({"operation":"session_revoke"}), false),
        (serde_json::json!({"operation":"publish"}), false),
        (serde_json::json!({"operation":"link"}), false),
    ] {
        let mut challenge = ChallengeRequest {
            endpoint: ProofEndpoint::MeshAdmission,
            key_id: "fixture".into(),
            body,
        };
        assert_eq!(cleanup_challenge(&challenge), allowed);
        challenge.endpoint = ProofEndpoint::GetEntitlements;
        assert!(!cleanup_challenge(&challenge));
    }
}
