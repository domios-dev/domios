use super::*;

#[test]
fn recall_priority_search_response_preserves_fts_hits_and_flags_degradation() {
    let hits = serde_json::json!({"hits":[{"id":"fts-hit"}]});
    assert_eq!(search_status(hits.clone(), false), hits);
    let degraded = search_status(hits.clone(), true);
    assert_eq!(degraded["hits"], hits["hits"]);
    assert_eq!(degraded["degraded"], "semantic pending");
}

#[test]
fn decision_http_search_does_not_block_runtime() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (release, signal) = std::sync::mpsc::channel::<()>();
    let output = runtime
        .block_on(async move {
            let search = run_blocking_search(move || {
                signal.recv().unwrap();
                Ok(serde_json::json!({ "hits": [] }))
            });
            let releaser = tokio::spawn(async move { release.send(()).unwrap() });
            let output = search.await;
            releaser.await.unwrap();
            output
        })
        .unwrap();
    assert_eq!(output, serde_json::json!({ "hits": [] }));
}

#[test]
fn search_errors_map_to_http_status() {
    for (error, expected) in [
        (
            SearchError::InvalidParams("invalid".into()),
            StatusCode::BAD_REQUEST,
        ),
        (
            SearchError::Internal("failed".into()),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ] {
        assert_eq!(search_error_http(error).0, expected);
    }
}
