use super::*;

struct Dropped(Arc<Notify>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[tokio::test]
async fn cancelled_read_drops_handler_resources_but_cancelled_mutation_finishes_once() {
    let (_fixture, state) = fixture();
    let entered = Arc::new(Notify::new());
    let dropped = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let finished = Arc::new(Notify::new());
    let count = Arc::new(AtomicUsize::new(0));
    let router = app(
        Router::new()
            .route(
                "/api/read-fixture",
                get({
                    let entered = entered.clone();
                    let dropped = dropped.clone();
                    move || {
                        let entered = entered.clone();
                        let dropped = dropped.clone();
                        async move {
                            let _resource = Dropped(dropped);
                            entered.notify_one();
                            std::future::pending::<()>().await;
                            "unreachable"
                        }
                    }
                }),
            )
            .route(
                "/api/mutation-fixture",
                post({
                    let entered = entered.clone();
                    let release = release.clone();
                    let finished = finished.clone();
                    let count = count.clone();
                    move || {
                        let entered = entered.clone();
                        let release = release.clone();
                        let finished = finished.clone();
                        let count = count.clone();
                        async move {
                            count.fetch_add(1, Ordering::SeqCst);
                            entered.notify_one();
                            release.notified().await;
                            finished.notify_one();
                            "done"
                        }
                    }
                }),
            ),
        state,
    );
    let (session, body) = open().await;
    for (id, method, path) in [
        ("read", "GET", "/api/read-fixture"),
        ("mutation", "POST", "/api/mutation-fixture"),
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(request(&session, id, method, path, Body::empty()))
                .await
                .unwrap()
                .status(),
            StatusCode::ACCEPTED
        );
        entered.notified().await;
        assert!(
            stream_mux_cancel_request(headers(), Path((session.clone(), id.into())))
                .await
                .is_ok()
        );
        if id == "read" {
            tokio::time::timeout(Duration::from_secs(2), dropped.notified())
                .await
                .unwrap();
        }
    }
    drop(body);
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), finished.notified())
        .await
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
