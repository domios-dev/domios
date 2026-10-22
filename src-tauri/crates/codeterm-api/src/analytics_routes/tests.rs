use super::*;

#[tokio::test]
async fn analytics_routes_post_refuses_before_event_handler() {
    use axum::{body::Body, http::Request, routing::post, Router};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tower::ServiceExt;
    let recorded = Arc::new(AtomicBool::new(false));
    let captured = recorded.clone();
    let router = Router::new().route(
        "/api/analytics/event",
        post(move |headers: HeaderMap| async move {
            authorize(
                &headers,
                Some("fixture-token"),
                Some(&CallerIdentity::Local),
            )?;
            captured.store(true, Ordering::SeqCst);
            Ok::<_, Error>(StatusCode::OK)
        }),
    );
    let response = router
        .oneshot(
            Request::post("/api/analytics/event")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!recorded.load(Ordering::SeqCst));
}

#[test]
fn analytics_routes_unauthenticated_post_refused() {
    assert_eq!(
        authorize(
            &HeaderMap::new(),
            Some("fixture-token"),
            Some(&CallerIdentity::Local)
        )
        .unwrap_err()
        .0,
        StatusCode::UNAUTHORIZED
    );
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer wrong".parse().unwrap());
    assert_eq!(
        authorize(
            &headers,
            Some("fixture-token"),
            Some(&CallerIdentity::Local)
        )
        .unwrap_err()
        .0,
        StatusCode::UNAUTHORIZED
    );
    headers.insert("authorization", "Bearer fixture-token".parse().unwrap());
    assert!(authorize(
        &headers,
        Some("fixture-token"),
        Some(&CallerIdentity::Local)
    )
    .is_ok());
    assert_eq!(
        authorize(&headers, Some("fixture-token"), None)
            .unwrap_err()
            .0,
        StatusCode::FORBIDDEN
    );
}

#[test]
fn client_events_allow_only_plan_views_and_features() {
    use codeterm_analytics::{Feature, Platform, Version};
    for event in [
        Event::AppStarted {
            version: Version::new(1, 15, 2),
            platform: Platform::Windows,
            mesh_machines: 1,
            workspaces: 1,
        },
        Event::AppFirstRun {
            version: Version::new(1, 15, 2),
            platform: Platform::Windows,
        },
        Event::AppOpened {},
        Event::AnalyticsOptedIn {},
        Event::PlanCreated {},
    ] {
        assert_eq!(
            admit_client_event(&event).unwrap_err().0,
            StatusCode::FORBIDDEN
        );
    }
    assert!(admit_client_event(&Event::PlanViewed {}).is_ok());
    let feature = Feature::MemorySaved;
    assert!(admit_client_event(&Event::FeatureUsed { feature }).is_ok());
}
