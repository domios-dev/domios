use super::*;
use axum::{routing::get, Router};
use tower::ServiceExt;

async fn call(status: StatusCode, body: &'static str) -> (StatusCode, serde_json::Value, String) {
    let router = Router::new()
        .route("/", get(move || async move { (status, body) }))
        .layer(axum::middleware::from_fn(typed_plan_refusal));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or_default(),
        text,
    )
}

#[tokio::test]
async fn exact_quota_refusals_become_typed_403() {
    for (body, code) in [
        (
            "free_limit_reached: Free plan allows 35 tabs",
            FREE_LIMIT_REACHED,
        ),
        (
            "plan_limit_locked: ws-3 is locked because it is above your plan's limit",
            PLAN_LIMIT_LOCKED,
        ),
    ] {
        let (status, json, _) = call(StatusCode::INTERNAL_SERVER_ERROR, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(json["error"]["code"], code);
        assert!(json["error"]["message"].as_str().unwrap().starts_with(code));
    }
}

#[tokio::test]
async fn other_errors_and_successes_pass_through_unchanged() {
    let (status, _, text) = call(StatusCode::INTERNAL_SERVER_ERROR, "disk full").await;
    assert_eq!(
        (status, text.as_str()),
        (StatusCode::INTERNAL_SERVER_ERROR, "disk full")
    );
    let (status, _, text) = call(StatusCode::OK, "free_limit_reached is a word").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text, "free_limit_reached is a word");
}

#[tokio::test]
async fn echoed_codes_and_partial_prefixes_are_not_refusals() {
    for body in [
        "Failed to create tab: free_limit_reached: echoed",
        "plan_limit_locked_extra: echoed",
        "unrelated plan_limit_locked: echoed",
    ] {
        let (status, _, text) = call(StatusCode::INTERNAL_SERVER_ERROR, body).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(text, body);
    }
    let (status, json, _) = call(
        StatusCode::BAD_REQUEST,
        r#"{"error":"plan_limit_locked: opaque"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], PLAN_LIMIT_LOCKED);
    assert_eq!(json["error"]["message"], "plan_limit_locked: opaque");
}
