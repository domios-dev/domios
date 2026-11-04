use super::*;

#[tokio::test]
async fn held_refresh_is_detached_coalesced_and_released_for_the_next_request() {
    static REFRESH: Refresh = Refresh::new();
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, receive) = tokio::sync::oneshot::channel();
    let operation = REFRESH
        .start(async move {
            entered.send(()).unwrap();
            receive.await.unwrap();
        })
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), started)
        .await
        .unwrap()
        .unwrap();
    assert!(!operation.is_finished());
    assert!(REFRESH
        .start(async { panic!("a second request must not queue a refresh") })
        .is_none());
    release.send(()).unwrap();
    operation.await.unwrap();
    REFRESH
        .start(async {})
        .expect("completed refresh must release admission")
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_background_task_releases_admission() {
    static REFRESH: Refresh = Refresh::new();
    let operation = REFRESH.start(std::future::pending()).unwrap();
    operation.abort();
    assert!(operation.await.unwrap_err().is_cancelled());
    REFRESH
        .start(async {})
        .expect("cancelled refresh must release admission")
        .await
        .unwrap();
}
