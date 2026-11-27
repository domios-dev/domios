use super::*;

mod outages;

#[test]
fn route_scoped_budget_uses_the_longer_deadline_and_isolates_unrelated_routes() {
    use super::super::{cloud_provisioning::ROUTE_CLOUD_DELETE, wire::*};
    let admission = budget_for("budget-fixture", ROUTE_ADMISSION);
    let challenge = budget_for("budget-fixture", ROUTE_CHALLENGE);
    let cloud_delete = budget_for("budget-fixture", ROUTE_CLOUD_DELETE);
    assert!(Arc::ptr_eq(&admission, &challenge));
    assert!(!Arc::ptr_eq(&admission, &cloud_delete));
    assert!(!Arc::ptr_eq(
        &admission,
        &budget_for("another-budget-fixture", ROUTE_ADMISSION)
    ));
    let start = Instant::now();
    let mut gate = admission.try_lock().unwrap();
    gate.limited(ROUTE_ADMISSION, start, 45);
    assert_eq!(gate.remaining(start), Duration::from_secs(45));
    assert!(challenge.try_lock().is_err());
    assert_eq!(
        cloud_delete.try_lock().unwrap().remaining(start),
        Duration::ZERO
    );
    drop(gate);
    let mut gate = challenge.try_lock().unwrap();
    assert_eq!(
        gate.remaining(start + Duration::from_secs(44)),
        Duration::from_secs(1)
    );
    gate.limited(ROUTE_ADMISSION, start + Duration::from_secs(45), 0);
    assert_eq!(
        gate.remaining(start + Duration::from_secs(45)),
        Duration::from_secs(2)
    );
    for _ in 0..400 {
        gate.limited(ROUTE_ADMISSION, start + Duration::from_secs(45), 0);
    }
    assert_eq!(
        gate.remaining(start + Duration::from_secs(45)),
        Duration::from_secs(300)
    );
    gate.success(ROUTE_ADMISSION, start + Duration::from_secs(345));
    assert_eq!(
        gate.remaining(start + Duration::from_secs(345)),
        Duration::ZERO
    );
    assert!(!gate.is_throttled());
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
    let budget = budget_for(uid, "admission");
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
            budget_for(&uid, route)
                .lock()
                .await
                .limited(route, clock.now(), 45);
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

struct DeadlineClock {
    uid: String,
    route: String,
    time: ClockPort,
}
#[async_trait::async_trait]
impl Clock for DeadlineClock {
    fn now(&self) -> Instant {
        self.time.now()
    }
    async fn wait(&self, delay: Duration) {
        assert!(
            budget_for(&self.uid, &self.route).try_lock().is_ok(),
            "budget is unlocked during wait"
        );
        self.time.wait(delay).await;
        tokio::time::advance(delay).await;
    }
}
struct DeadlineTransport {
    clock: DeadlineClock,
    hang: bool,
    calls: std::sync::atomic::AtomicU64,
}
#[async_trait::async_trait]
impl super::super::callable::CallableTransport for DeadlineTransport {
    fn retry_clock(&self) -> &dyn Clock {
        &self.clock
    }
    async fn call(
        &self,
        _: &str,
        _: &str,
        _: serde_json::Value,
    ) -> Result<serde_json::Value, super::super::callable::CallableFailure> {
        assert!(
            budget_for(&self.clock.uid, &self.clock.route)
                .try_lock()
                .is_ok(),
            "budget is unlocked during transport"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.hang {
            std::future::pending::<()>().await;
        }
        Ok(serde_json::json!({"result":{"nonce":"n.a.b"}}))
    }
}

#[tokio::test(start_paused = true)]
async fn mesh_budget_wait_is_outside_call_deadline_and_locks_cover_neither_wait_nor_io() {
    use super::super::{
        callable::CallableRegistry, lifecycle::*, ports::*, state::AccountMeshRefusal,
        tests::support::*, wire::*,
    };
    for hang in [false, true] {
        let uid = format!("mesh-budget-deadline-{hang}");
        let clock = DeadlineClock {
            uid: uid.clone(),
            route: ROUTE_CHALLENGE.into(),
            time: ClockPort::new(),
        };
        budget_for(&uid, ROUTE_ADMISSION)
            .lock()
            .await
            .limited(ROUTE_ADMISSION, clock.now(), 45);
        let registry = Arc::new(CallableRegistry::new(DeadlineTransport {
            clock,
            hang,
            calls: 0.into(),
        }));
        let harness = Harness::new().await;
        let mesh = AccountMesh::new(
            AccountMeshConfig {
                installation_id: crate::InstallationId::parse(INSTALLATION.into()).unwrap(),
                default_display_name: None,
                trust: None,
                call_deadline: Duration::from_secs(15),
                withdrawal_deadline: Duration::from_secs(10),
                schedule_leases: false,
            },
            AccountMeshPorts {
                registry: registry.clone(),
                sessions: harness.sessions.clone(),
                keys: harness.keys.clone(),
                store: harness.store.clone(),
                endpoint: harness.endpoint.clone(),
                transport: harness.transport.clone(),
                clock: harness.clock.clone(),
            },
        );
        let request = ChallengeRequest {
            endpoint: ProofEndpoint::MeshAdmission,
            key_id: "a".repeat(64),
            body: serde_json::json!({}),
        };
        let session = session(&uid);
        let start = tokio::time::Instant::now();
        let result = mesh.call(registry.challenge(&session, &request)).await;
        assert_eq!(
            registry
                .transport()
                .calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            start.elapsed(),
            Duration::from_secs(if hang { 60 } else { 45 })
        );
        assert_eq!(
            result,
            if hang {
                Err(AccountMeshRefusal::Offline)
            } else {
                Ok("n.a.b".into())
            }
        );
    }
}

struct BurstTransport {
    entered: tokio::sync::mpsc::UnboundedSender<String>,
    release: tokio::sync::Semaphore,
    calls: parking_lot::Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl super::super::callable::CallableTransport for BurstTransport {
    async fn call(
        &self,
        route: &str,
        _: &str,
        _: serde_json::Value,
    ) -> Result<serde_json::Value, super::super::callable::CallableFailure> {
        self.calls.lock().push(route.into());
        self.entered.send(route.into()).unwrap();
        self.release.acquire().await.unwrap().forget();
        if route == super::super::wire::ROUTE_ADMISSION {
            Err(super::super::callable::CallableFailure::Status {
                status: 429,
                retry_after_seconds: Some(90),
                body: None,
            })
        } else {
            Ok(serde_json::json!({"result":true}))
        }
    }
}

#[tokio::test]
async fn parked_same_family_burst_serializes_probes_and_rechecks_new_429_before_dispatch() {
    use super::super::{callable::CallableFailure, wire::*};
    let uid = "serialized-same-family-burst";
    let (waiting, mut waits) = tokio::sync::mpsc::unbounded_channel();
    let clock = ParkedClock {
        time: ClockPort::new(),
        waiting,
        release: tokio::sync::Semaphore::new(0),
    };
    let (entered, mut calls) = tokio::sync::mpsc::unbounded_channel();
    let transport = BurstTransport {
        entered,
        release: tokio::sync::Semaphore::new(0),
        calls: Default::default(),
    };
    budget_for(uid, ROUTE_ADMISSION)
        .lock()
        .await
        .limited(ROUTE_ADMISSION, clock.now(), 45);
    let admission_guard = guard();
    let challenge_guard = guard();
    let enroll_guard = guard();
    let control = async {
        waits.recv().await.unwrap();
        assert!(budget_for(uid, ROUTE_ADMISSION).try_lock().is_ok());
        assert!(calls.try_recv().is_err());
        clock.release.add_permits(1);
        assert_eq!(calls.recv().await.unwrap(), ROUTE_ADMISSION);
        assert!(calls.try_recv().is_err());
        enroll_guard.authority.send_replace(None);
        transport.release.add_permits(1);
        waits.recv().await.unwrap();
        assert_eq!(
            budget_for(uid, ROUTE_ADMISSION)
                .lock()
                .await
                .remaining(clock.now()),
            Duration::from_secs(90)
        );
        assert!(
            calls.try_recv().is_err(),
            "queued routes in the same family cannot dispatch before the new deadline"
        );
        clock.release.add_permits(1);
        assert_eq!(calls.recv().await.unwrap(), ROUTE_CHALLENGE);
        transport.release.add_permits(1);
    };
    let (admission, challenge, enroll, ()) = tokio::join!(biased;
        run(&transport, uid, ROUTE_ADMISSION, "", serde_json::Value::Null, &admission_guard, &clock),
        run(&transport, uid, ROUTE_CHALLENGE, "", serde_json::Value::Null, &challenge_guard, &clock),
        run(&transport, uid, ROUTE_ENROLL_KEY, "", serde_json::Value::Null, &enroll_guard, &clock),
        control,
    );
    assert!(matches!(
        admission,
        Err(CallableFailure::Status { status: 429, .. })
    ));
    assert!(challenge.is_ok());
    assert_eq!(enroll, Err(CallableFailure::Retired));
    assert_eq!(*transport.calls.lock(), [ROUTE_ADMISSION, ROUTE_CHALLENGE]);
    assert_eq!(*clock.time.waits.lock(), [45, 90].map(Duration::from_secs));
    let live = guard();
    let release = async {
        assert_eq!(calls.recv().await.unwrap(), ROUTE_ENROLL_KEY);
        transport.release.add_permits(1);
    };
    let (result, ()) = tokio::join!(
        run(
            &transport,
            uid,
            ROUTE_ENROLL_KEY,
            "",
            serde_json::Value::Null,
            &live,
            &clock
        ),
        release,
    );
    assert!(
        result.is_ok(),
        "retired waiters leave the next admission available"
    );
    assert_eq!(
        *transport.calls.lock(),
        [ROUTE_ADMISSION, ROUTE_CHALLENGE, ROUTE_ENROLL_KEY]
    );
}

#[tokio::test]
async fn rate_limit_on_one_route_does_not_delay_or_serialize_unrelated_routes() {
    use super::super::{
        callable::CallableFailure,
        cloud_provisioning::{ROUTE_CLOUD_CREATE, ROUTE_CLOUD_DELETE, ROUTE_CLOUD_USAGE},
        wire::*,
    };
    let uid = "budget-isolation-fixture";
    let (waiting, mut waits) = tokio::sync::mpsc::unbounded_channel();
    let clock = ParkedClock {
        time: ClockPort::new(),
        waiting,
        release: tokio::sync::Semaphore::new(0),
    };
    let (entered, mut calls) = tokio::sync::mpsc::unbounded_channel();
    let transport = BurstTransport {
        entered,
        release: tokio::sync::Semaphore::new(0),
        calls: Default::default(),
    };
    // Simulate a 296s 429 on accountMeshAdmission.
    budget_for(uid, ROUTE_ADMISSION)
        .lock()
        .await
        .limited(ROUTE_ADMISSION, clock.now(), 296);
    let parked_guard = guard();
    let unrelated_guard = guard();

    let test_unrelated = async {
        // Wait until the accountMeshAdmission caller is parked inside its 296s backoff.
        waits.recv().await.unwrap();

        // Unrelated cloud routes and concurrent healthy calls must dispatch immediately with zero wait.
        let delete_fut = run(
            &transport,
            uid,
            ROUTE_CLOUD_DELETE,
            "",
            serde_json::Value::Null,
            &unrelated_guard,
            &clock,
        );
        let usage_fut = run(
            &transport,
            uid,
            ROUTE_CLOUD_USAGE,
            "",
            serde_json::Value::Null,
            &unrelated_guard,
            &clock,
        );
        let friends_fut = run(
            &transport,
            uid,
            "friendsList",
            "",
            serde_json::Value::Null,
            &unrelated_guard,
            &clock,
        );
        let release_concurrent = async {
            // All 3 unrelated routes enter transport concurrently without waiting on each other or on admission.
            let mut seen = Vec::new();
            for _ in 0..3 {
                seen.push(calls.recv().await.unwrap());
            }
            seen.sort();
            assert_eq!(
                seen,
                vec![
                    ROUTE_CLOUD_DELETE.to_owned(),
                    "friendsList".to_owned(),
                    ROUTE_CLOUD_USAGE.to_owned(),
                ]
            );
            assert!(clock.time.waits.lock().is_empty());
            transport.release.add_permits(3);
        };
        let (delete_res, usage_res, friends_res, ()) =
            tokio::join!(delete_fut, usage_fut, friends_fut, release_concurrent);
        assert!(delete_res.is_ok());
        assert!(usage_res.is_ok());
        assert!(friends_res.is_ok());

        // A 429 on createManagedCloudMachine also does not delay deleteManagedCloudMachine.
        budget_for(uid, ROUTE_CLOUD_CREATE).lock().await.limited(
            ROUTE_CLOUD_CREATE,
            clock.now(),
            120,
        );
        let delete_after_create_429 = run(
            &transport,
            uid,
            ROUTE_CLOUD_DELETE,
            "",
            serde_json::Value::Null,
            &unrelated_guard,
            &clock,
        );
        let release_delete = async {
            assert_eq!(calls.recv().await.unwrap(), ROUTE_CLOUD_DELETE);
            assert!(clock.time.waits.lock().is_empty());
            transport.release.add_permits(1);
        };
        let (delete_again, ()) = tokio::join!(delete_after_create_429, release_delete);
        assert!(delete_again.is_ok());

        // Admission's own 296s backoff remains untouched while unrelated routes succeeded.
        assert_eq!(
            budget_for(uid, ROUTE_ADMISSION)
                .lock()
                .await
                .remaining(clock.now()),
            Duration::from_secs(296)
        );
        parked_guard.authority.send_replace(None);
    };

    let (parked_res, ()) = tokio::join!(biased;
        run(&transport, uid, ROUTE_ADMISSION, "", serde_json::Value::Null, &parked_guard, &clock),
        test_unrelated,
    );
    assert_eq!(parked_res, Err(CallableFailure::Retired));
    assert!(clock.time.waits.lock().is_empty());
}

struct CleanupTransport {
    calls: parking_lot::Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl super::super::callable::CallableTransport for CleanupTransport {
    async fn call(
        &self,
        route: &str,
        _: &str,
        _: serde_json::Value,
    ) -> Result<serde_json::Value, super::super::callable::CallableFailure> {
        self.calls.lock().push(route.into());
        Ok(serde_json::json!({"result":{
            "kind":"withdrawn", "installation_id":"cleanup-installation",
            "previous_metadata_revision":2, "metadata_revision":3,
            "slot_revision":2, "registry_revision":7,
        }}))
    }
}

#[tokio::test(start_paused = true)]
async fn signout_withdrawal_finishes_inside_deadline_while_account_probe_is_parked() {
    use super::super::{
        callable::CallableRegistry, ports::MeshRegistry, tests::support::session, wire::*,
    };
    let uid = "cleanup-priority-budget";
    let (waiting, mut waits) = tokio::sync::mpsc::unbounded_channel();
    let clock = ParkedClock {
        time: ClockPort::new(),
        waiting,
        release: tokio::sync::Semaphore::new(0),
    };
    budget_for(uid, ROUTE_ADMISSION)
        .lock()
        .await
        .limited(ROUTE_ADMISSION, clock.now(), 300);
    let registry = CallableRegistry::new(CleanupTransport {
        calls: Default::default(),
    });
    let parked_guard = guard();
    let cleanup = async {
        waits.recv().await.unwrap();
        let session = session(uid);
        let envelope = SignedEnvelope {
            request: MeshAdmissionBody {
                schema_version: 1,
                installation_id: "cleanup-installation".into(),
                request_id: "a".repeat(64),
                signed_registry: None,
                endpoint: None,
                own_row: None,
                default_display_name: None,

                operation: MeshOperation::Withdraw {
                    expected_metadata_revision: 2,
                },
            },
            installation_proof: InstallationProof {
                schema_version: 1,
                key_id: "b".repeat(64),
                generation: 1,
                timestamp: 1_800_000_000,
                nonce: "nonce".into(),
                signature: "signature".into(),
            },
        };
        let start = tokio::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            registry.admission(&session, &envelope),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(
            result,
            MeshResult::Withdrawn(WithdrawnResult {
                signed_registry: None,
                metadata_revision: 3,
                ..
            })
        ));
        assert_eq!(start.elapsed(), Duration::ZERO);
        assert_eq!(
            budget_for(uid, ROUTE_ADMISSION)
                .lock()
                .await
                .remaining(clock.now()),
            Duration::from_secs(300)
        );
        parked_guard.authority.send_replace(None);
    };
    let (parked, ()) = tokio::join!(biased;
        run(registry.transport(), uid, ROUTE_CHALLENGE, "", serde_json::Value::Null, &parked_guard, &clock),
        cleanup,
    );
    assert_eq!(
        parked,
        Err(super::super::callable::CallableFailure::Retired)
    );
    assert_eq!(*registry.transport().calls.lock(), [ROUTE_ADMISSION]);
}
