use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn heartbeat_projection_waiting_on_configuration_keeps_rest_worker_responsive() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = app_state(&fixture);
    let router = crate::routes::create_router(state.clone());
    let config = state.config.clone();
    let held = HeldDependency::new(move |locked, release| {
        let _guard = config.lock();
        locked.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(3)).unwrap();
    })
    .await;
    let mut heartbeat = Box::pin(crate::delayed_send_routes::system_heartbeats_for_tab(
        state.clone(),
        tab,
    ));
    assert!(heartbeat.as_mut().now_or_never().is_none());
    health_responds(router).await;
    assert!(state.config.try_lock().is_none());
    assert!(heartbeat.as_mut().now_or_never().is_none());
    drop(held);
    let response = heartbeat.await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!([])
    );
}
