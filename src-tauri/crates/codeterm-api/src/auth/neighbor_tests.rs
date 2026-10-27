use super::*;

#[tokio::test]
async fn concierge_capability_refusals_keep_typed_http_403() {
    for reason in [
        "neighbor_memory_not_allowed",
        "neighbor_memory_read_only",
        "neighbor_secrets_not_allowed",
        "neighbor_files_not_allowed",
        "tab_requests_not_allowed",
    ] {
        let response = neighbor_capability_failure(format!("{reason}: opaque"));
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["reason"], reason);
        assert_eq!(value["code"], reason);
    }
}
