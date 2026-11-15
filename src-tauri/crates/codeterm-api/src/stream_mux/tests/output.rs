use super::*;

#[tokio::test]
async fn replayable_output_paths_deliver_events_larger_than_channel_depth_whole() {
    async fn replay() -> axum::response::Response {
        axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(Body::from(format!("data: {}\n\n", "x".repeat(128 * 1024))))
            .unwrap()
    }
    for path in [
        "/api/output/stream",
        "/api/tabs/own/output/stream",
        "/api/mesh/relay-stream?peer=peer&path=%2Fapi%2Ftabs%2Fown%2Foutput%2Fstream",
    ] {
        let (_fixture, state) = fixture();
        let router = app(
            Router::new()
                .route("/api/output/stream", get(replay))
                .route("/api/tabs/{id}/output/stream", get(replay))
                .route("/api/mesh/relay-stream", get(replay)),
            state.clone(),
        );
        let dispatcher = dispatch::Dispatcher::new(router, state.svc.clone());
        let (session, mut body) = open().await;
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/stream-mux/{session}/channels"))
            .body(Body::from(
                serde_json::json!({"channel":"output","path":path}).to_string(),
            ))
            .unwrap();
        request.headers_mut().extend(headers());
        let _opened = stream_mux_open_channel(Extension(dispatcher), Path(session), request)
            .await
            .unwrap();
        assert_eq!(frame(&mut body).await["status"], 200);
        let mut received = String::new();
        loop {
            let frame = frame(&mut body).await;
            if let Some(end) = frame.get("end") {
                assert_eq!(end["reason"], "eof", "{path}");
                break;
            }
            received.push_str(frame["chunk"].as_str().unwrap());
        }
        assert_eq!(
            received,
            format!("data: {}\n\n", "x".repeat(128 * 1024)),
            "{path}"
        );
    }
}
