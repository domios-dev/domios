use super::*;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::get as route_get,
    Router,
};
use tower::ServiceExt;

#[tokio::test]
async fn projection_get_accepts_browser_list_window_and_numeric_cursor_queries() {
    let (fixture, plan_id, _, _, _, _) = crate::plan_routes::tasks::tests::update_fixture();
    let router = Router::new()
        .route("/api/plans/{id}/projection", route_get(get))
        .with_state(crate::transport_tabs::fixture::app_state(&fixture));
    for query in [
        "view=list&page_size=25&status=all&search=",
        "view=list&page_size=25&status=all&search=&after_position=0&after_task_id=task_0000",
        "view=task_metadata&page_size=25&status=all&search=",
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/plans/{plan_id}/projection?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "query: {query}");
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(value["id"], plan_id);
        assert!(value["tasks"].as_array().unwrap().len() <= 25);
        assert!(value["task_window"]["total"].is_number());
    }
    let uri = format!("/api/plans/{plan_id}/projection?view=list&page_size=invalid")
        .parse()
        .unwrap();
    assert!(Query::<ProjectionQuery>::try_from_uri(&uri).is_err());
}
