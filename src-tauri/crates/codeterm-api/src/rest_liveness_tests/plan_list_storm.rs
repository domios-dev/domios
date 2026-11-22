use super::*;
use axum::extract::{Query, State};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn plan_list_storm_never_exhausts_blocking_pool_for_unrelated_routes() {
    // Allow just 2 more blocking threads than PLAN_LIST_CONCURRENCY so any unbounded
    // fan-out of 32 concurrent GET /api/plans calls would immediately starve the pool.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(10)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = ServiceFixture::new(MultiplexerState::empty());
        let state = app_state(&fixture);
        state.svc.mark_plans_startup_loaded();
        let router = crate::routes::create_router(state.clone());
        let plans = state.svc.plan_store.clone();

        let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            plans.with_database_held_for_test(|| {
                locked_tx.send(()).unwrap();
                let _ = release_rx.recv_timeout(Duration::from_secs(5));
            });
        });
        locked_rx.await.unwrap();

        let started = Arc::new(AtomicUsize::new(0));
        let mut storm = Vec::with_capacity(32);
        for _ in 0..32 {
            let state = state.clone();
            let started = started.clone();
            storm.push(tokio::spawn(async move {
                started.fetch_add(1, Ordering::Relaxed);
                crate::plan_routes::list(
                    State(state),
                    axum::http::HeaderMap::new(),
                    Query(Default::default()),
                )
                .await
            }));
        }
        // Let all 32 tasks reach the semaphore / blocking boundary.
        while started.load(Ordering::Relaxed) < 32 {
            tokio::task::yield_now().await;
        }
        tokio::task::yield_now().await;

        // Unrelated spawn_blocking work (same pool used by send/chat-send/todo routes)
        // AND /api/health must still answer immediately while all 32 plan-list calls wait.
        let unrelated = tokio::time::timeout(
            Duration::from_millis(250),
            tokio::task::spawn_blocking(|| 7 + 35),
        )
        .await
        .expect("plan list storm must never starve unrelated blocking routes")
        .unwrap();
        assert_eq!(unrelated, 42);

        let health = tokio::time::timeout(
            Duration::from_millis(250),
            router.oneshot(request("/api/health")),
        )
        .await
        .expect("health must answer during plan list storm")
        .unwrap();
        assert_eq!(health.status(), StatusCode::OK);

        // Dropping half the queued requests cancels them cleanly without leaking permits.
        for handle in storm.drain(..16) {
            handle.abort();
        }
        release_tx.send(()).unwrap();
        holder.join().unwrap();

        for handle in storm {
            let result = tokio::time::timeout(Duration::from_secs(3), handle)
                .await
                .expect("remaining plan list calls must drain cleanly after DB release")
                .unwrap();
            assert!(result.is_ok());
        }
    });
}
