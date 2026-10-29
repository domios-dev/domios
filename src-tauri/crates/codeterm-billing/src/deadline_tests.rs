use super::*;
use std::sync::atomic::Ordering;

const ISSUED: u64 = 1_700_000_000;

fn primed(store: Arc<MemoryGrants>, clock: Arc<dyn Fn() -> u64 + Send + Sync>) -> Billing {
    let billing = Billing::new(GrantReplies::new(vec![])).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        store,
        clock,
    );
    billing.prime(&session("fixture-user"));
    billing
}

#[test]
fn next_deadline_walks_refresh_then_lease_end_then_none() {
    let store = Arc::new(MemoryGrants::default());
    store_verified(&store, &paid_grant().grant.unwrap());
    let (time, clock) = clock(ISSUED + 60);
    let billing = primed(store, clock);
    assert_eq!(billing.next_deadline(), Some(ISSUED + 3_600));
    assert!(!billing.refresh_due());
    time.store(ISSUED + 3_600, Ordering::SeqCst);
    assert_eq!(billing.next_deadline(), Some(ISSUED + 86_400));
    assert!(billing.refresh_due());
    time.store(ISSUED + 86_400, Ordering::SeqCst);
    assert_eq!(billing.next_deadline(), None);
    assert!(!billing.refresh_due());
}

#[test]
fn checking_bound_is_a_deadline() {
    let store = Arc::new(MemoryGrants::default());
    *store.3.lock() = true;
    let (_, clock) = clock(ISSUED);
    let billing = primed(store, clock);
    assert_eq!(
        billing.next_deadline(),
        Some(ISSUED + CHECKING_DEADLINE_SECS)
    );
}

#[tokio::test(start_paused = true)]
async fn idle_driver_refreshes_and_announces_expiry_without_inventing_free_locks() {
    let store = Arc::new(MemoryGrants::default());
    store_verified(&store, &paid_grant().grant.unwrap());
    let start = tokio::time::Instant::now();
    let clock: Arc<dyn Fn() -> u64 + Send + Sync> =
        Arc::new(move || ISSUED + 60 + start.elapsed().as_secs());
    let billing = primed(store.clone(), clock.clone());
    assert!(billing.limit_gate().summary().workspaces.is_none());

    let (ticks, mut seen) = tokio::sync::mpsc::unbounded_channel();
    let driver = billing.clone();
    let task = tokio::spawn(async move {
        driver
            .drive_deadlines(|tick| {
                let driver = driver.clone();
                let ticks = ticks.clone();
                let clock = clock.clone();
                async move {
                    if tick.refresh_due {
                        driver
                            .refresh(
                                Some(&session("fixture-user")),
                                RefreshEvent::Deadline,
                                std::time::Instant::now(),
                            )
                            .await;
                    }
                    let _ = ticks.send((clock(), tick, driver.limit_gate().summary()));
                }
            })
            .await;
    });

    let (at, tick, gate) = seen.recv().await.unwrap();
    assert_eq!((at, tick.refresh_due), (ISSUED + 3_600, true));
    assert!(gate.workspaces.is_none());
    let (at, tick, gate) = seen.recv().await.unwrap();
    assert_eq!((at, tick.refresh_due), (ISSUED + 86_400, false));
    assert_eq!(
        gate.workspaces,
        codeterm_entitlements::COMPILED_FREE_LIMITS.workspaces.max
    );
    assert!(!gate.enforcing);
    assert!(store.4.lock().as_ref().unwrap().observed_at >= ISSUED + 86_400);
    task.abort();
}

#[tokio::test(start_paused = true)]
async fn a_new_grant_rearms_the_driver() {
    let store = Arc::new(MemoryGrants::default());
    let start = tokio::time::Instant::now();
    let clock: Arc<dyn Fn() -> u64 + Send + Sync> =
        Arc::new(move || ISSUED + 60 + start.elapsed().as_secs());
    let billing = Billing::new(GrantReplies::new(vec![Ok(paid_grant())])).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        store,
        clock.clone(),
    );
    let user = session("fixture-user");
    billing.prime(&user);
    assert_eq!(billing.next_deadline(), None);
    let (ticks, mut seen) = tokio::sync::mpsc::unbounded_channel();
    let driver = billing.clone();
    let task = tokio::spawn(async move {
        driver
            .drive_deadlines(|_| {
                let ticks = ticks.clone();
                let at = clock();
                async move {
                    let _ = ticks.send(at);
                }
            })
            .await;
    });
    tokio::task::yield_now().await;
    billing
        .refresh(Some(&user), RefreshEvent::SignIn, std::time::Instant::now())
        .await;
    assert_eq!(seen.recv().await.unwrap(), ISSUED + 3_600);
    task.abort();
}

#[test]
fn captured_paid_mux_gate_does_not_invent_free_on_wall_jump() {
    let store = Arc::new(MemoryGrants::default());
    store_verified(&store, &paid_grant().grant.unwrap());
    let (wall, clock) = clock(ISSUED + 60);
    let billing = primed(store, clock);
    let gate = billing.limit_gate();
    assert_eq!(gate.summary().workspaces, None);
    wall.store(ISSUED + 3_600, Ordering::SeqCst);
    assert_eq!(gate.summary().workspaces, None);
    wall.store(ISSUED + 86_400, Ordering::SeqCst);
    assert_eq!(gate.summary().workspaces, None);
}

#[test]
fn paid_gate_captures_access_without_expiry_derived_free_locks() {
    let store = Arc::new(MemoryGrants::default());
    store_verified(&store, &paid_grant().grant.unwrap());
    let time = Arc::new(std::sync::atomic::AtomicU64::new(ISSUED + 3_600));
    let clock = time.clone();
    let billing = primed(
        store,
        Arc::new(move || clock.fetch_add(1, Ordering::SeqCst)),
    );
    time.store(ISSUED + 86_399, Ordering::SeqCst);
    assert_eq!(billing.limit_gate().summary().workspaces, None);
}
