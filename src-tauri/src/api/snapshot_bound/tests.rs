use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Barrier;
use tokio::task::JoinSet;

/// Bounded failure deadline for an event wait — never a readiness signal. A test
/// that reaches it has deadlocked, which is the failure being guarded against.
const DEADLINE: Duration = Duration::from_secs(30);

/// The storm: 1,028-1,050 concurrent `GET /api/mux/state` in the surviving runs.
const BURST: usize = 1_050;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_snapshot_burst_leaves_the_daemon_alive_with_no_more_than_the_bound_in_flight() {
    let bound = SnapshotBound::new();
    let peak = Arc::new(AtomicUsize::new(0));
    let admitted = Arc::new(AtomicUsize::new(0));
    let refused = Arc::new(AtomicUsize::new(0));

    let mut burst = JoinSet::new();
    for _ in 0..BURST {
        let bound = bound.clone();
        let peak = peak.clone();
        let admitted = admitted.clone();
        let refused = refused.clone();
        burst.spawn(async move {
            match bound.try_enter() {
                Some(permit) => {
                    admitted.fetch_add(1, Ordering::AcqRel);
                    peak.fetch_max(bound.in_flight(), Ordering::AcqRel);
                    drop(permit);
                }
                None => {
                    refused.fetch_add(1, Ordering::AcqRel);
                }
            }
        });
    }

    // Every request is answered — the daemon does not die and does not stall.
    let joined = tokio::time::timeout(DEADLINE, async {
        let mut count = 0;
        while let Some(result) = burst.join_next().await {
            result.expect("no request task may panic");
            count += 1;
        }
        count
    })
    .await
    .expect("the burst must drain, not deadlock");

    assert_eq!(joined, BURST, "every request in the burst is answered");
    assert_eq!(
        admitted.load(Ordering::Acquire) + refused.load(Ordering::Acquire),
        BURST,
        "every request is either admitted or refused; none is queued"
    );
    assert!(
        peak.load(Ordering::Acquire) <= MAX_CONCURRENT_STATE_SNAPSHOTS,
        "peak concurrent snapshots {} exceeded the bound {}",
        peak.load(Ordering::Acquire),
        MAX_CONCURRENT_STATE_SNAPSHOTS
    );
    assert_eq!(bound.in_flight(), 0, "no permit is leaked after the burst");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn traffic_under_the_bound_is_unaffected() {
    // Without this, the burst test above passes by refusing everything.
    let bound = SnapshotBound::new();
    let all_inside = Arc::new(Barrier::new(MAX_CONCURRENT_STATE_SNAPSHOTS));
    let refused = Arc::new(AtomicUsize::new(0));

    let mut requests = JoinSet::new();
    for _ in 0..MAX_CONCURRENT_STATE_SNAPSHOTS {
        let bound = bound.clone();
        let all_inside = all_inside.clone();
        let refused = refused.clone();
        requests.spawn(async move {
            match bound.try_enter() {
                // The barrier only releases once every one of them holds a permit
                // at the same instant, so admission is proven, not timed.
                Some(permit) => {
                    all_inside.wait().await;
                    drop(permit);
                }
                None => {
                    refused.fetch_add(1, Ordering::AcqRel);
                }
            }
        });
    }

    tokio::time::timeout(DEADLINE, async {
        while let Some(result) = requests.join_next().await {
            result.expect("no request task may panic");
        }
    })
    .await
    .expect("a burst exactly at the bound must all be admitted simultaneously");

    assert_eq!(
        refused.load(Ordering::Acquire),
        0,
        "traffic at or under the bound is never refused"
    );
}

#[test]
fn exceeding_the_bound_is_refused_rather_than_queued() {
    let bound = SnapshotBound::new();

    let held: Vec<_> = (0..MAX_CONCURRENT_STATE_SNAPSHOTS)
        .map(|i| {
            bound
                .try_enter()
                .unwrap_or_else(|| panic!("request {i} is under the bound and must be admitted"))
        })
        .collect();

    assert_eq!(bound.in_flight(), MAX_CONCURRENT_STATE_SNAPSHOTS);
    assert!(
        bound.try_enter().is_none(),
        "a request over the bound is answered, never enqueued"
    );

    drop(held);
    assert_eq!(bound.in_flight(), 0);
    assert!(
        bound.try_enter().is_some(),
        "capacity returns when a snapshot completes"
    );
}

#[test]
fn the_overload_answer_is_a_refusal_status_and_not_a_success() {
    assert_eq!(SNAPSHOT_OVERLOAD_STATUS, StatusCode::SERVICE_UNAVAILABLE);
    assert!(SNAPSHOT_OVERLOAD_STATUS.is_server_error());
    assert!(!SNAPSHOT_OVERLOAD_STATUS.is_success());
    assert!(!SNAPSHOT_OVERLOAD_BODY.is_empty());
}

#[test]
fn the_bound_is_a_limit_not_a_delay() {
    // try_enter returns immediately in both directions; nothing in this path waits
    // on elapsed time for capacity.
    let bound = SnapshotBound::with_limit(1);
    let permit = bound.try_enter().expect("first is admitted");
    assert!(bound.try_enter().is_none(), "second is refused at once");
    drop(permit);
    assert!(bound.try_enter().is_some(), "capacity is immediate");
}
