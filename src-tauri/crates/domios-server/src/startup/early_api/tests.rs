use super::*;
use axum::body::{to_bytes, Body};
use std::sync::atomic::{AtomicUsize, Ordering};

fn complete_service(api: &EarlyApi) {
    for phase in [
        "listener",
        "states",
        "providers",
        "service",
        "plans",
        "background",
        "platform",
    ] {
        api.complete(phase);
    }
}

fn request(method: Method, path: &str) -> Request {
    Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(
            "127.0.0.1:40000".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::empty())
        .unwrap()
}

async fn json(response: Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 16 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
async fn early_bind_health_answers_while_subsystems_are_starting() {
    let api = EarlyApi::new(None);
    api.complete("listener");
    let response = api
        .router()
        .oneshot(request(Method::GET, "/api/health"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["startup"]["state"], "starting");
    assert_eq!(body["startup"]["phases"]["listener"], "ready");
    for phase in ["integrity", "providers", "plans", "background"] {
        assert_eq!(body["startup"]["phases"][phase], "starting");
    }
}

#[tokio::test]
async fn starting_refusal_never_dispatches_or_replays_a_mutation() {
    let api = EarlyApi::new(None);
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    let full = Router::new().route(
        "/api/mutate",
        axum::routing::post(move || {
            handler_calls.fetch_add(1, Ordering::SeqCst);
            async { Json(serde_json::json!({ "accepted": true })) }
        }),
    );
    let listener_router = api.router();
    let response = listener_router
        .clone()
        .oneshot(request(Method::POST, "/api/mutate"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(response).await["error"]["code"], "starting");
    complete_service(&api);
    assert!(api.activate(full.clone()).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    api.complete("integrity");
    api.activate(full).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let response = listener_router
        .oneshot(request(Method::POST, "/api/mutate"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["accepted"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn integrity_failure_keeps_service_unavailable_and_publishes_failure() {
    let api = EarlyApi::new(None);
    let mut changes = api.changes.subscribe();
    complete_service(&api);
    changes.borrow_and_update();
    api.fail("integrity");
    changes.changed().await.unwrap();
    assert_eq!(changes.borrow().state, PhaseState::Failed);
    api.complete("integrity");
    assert!(api.activate(Router::new()).is_err());
    let response = api
        .router()
        .oneshot(request(Method::POST, "/api/mutate"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(response).await["error"]["code"], "startup_failed");
}

#[tokio::test]
async fn readiness_event_and_atomic_handoff_share_the_same_completion() {
    let api = EarlyApi::new(None);
    let listener_router = api.router();
    let mut changes = api.changes.subscribe();
    complete_service(&api);
    api.complete("integrity");
    changes.borrow_and_update();
    api.activate(Router::new().route("/api/probe", axum::routing::get(|| async { "ready" })))
        .unwrap();
    changes.changed().await.unwrap();
    assert_eq!(changes.borrow().state, PhaseState::Ready);
    let response = listener_router
        .clone()
        .oneshot(request(Method::GET, "/api/probe"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = listener_router
        .oneshot(request(Method::GET, "/api/startup/events"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let event = std::str::from_utf8(&bytes).unwrap();
    assert!(event.contains("event: startup-state"));
    let data = event
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(data).unwrap()["state"],
        "ready"
    );
}

#[tokio::test]
async fn startup_events_refuse_foreign_unauthenticated_callers() {
    let api = EarlyApi::new(Some("fixture-token".into()));
    let request = Request::builder()
        .uri("/api/startup/events")
        .extension(ConnectInfo(
            "192.0.2.1:40000".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::empty())
        .unwrap();
    let response = api.router().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    complete_service(&api);
    api.complete("integrity");
    api.activate(Router::new()).unwrap();
    let request = Request::builder()
        .uri("/api/startup/events")
        .header("authorization", "Bearer fixture-token")
        .extension(ConnectInfo(
            "192.0.2.1:40000".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        api.router().oneshot(request).await.unwrap().status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn early_health_only_exempts_loopback_from_authentication() {
    let api = EarlyApi::new(Some("fixture-token".into()));
    for address in ["192.0.2.1:40000", "[2001:db8::1]:40000"] {
        for token in [None, Some("wrong-token"), Some("fixture-token")] {
            let mut request = Request::builder()
                .uri("/api/health")
                .extension(ConnectInfo(address.parse::<SocketAddr>().unwrap()));
            if let Some(token) = token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            let response = api
                .router()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if token == Some("fixture-token") {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                }
            );
        }
    }
    assert_eq!(
        api.router()
            .oneshot(request(Method::GET, "/api/health"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn ready_health_delegates_authentication_to_the_active_router() {
    let api = EarlyApi::new(Some("startup-token".into()));
    complete_service(&api);
    api.complete("integrity");
    api.activate(Router::new().route(
        "/api/health",
        axum::routing::get(|| async { StatusCode::ACCEPTED }),
    ))
    .unwrap();
    let request = Request::builder()
        .uri("/api/health")
        .header("authorization", "Bearer replacement-token")
        .extension(ConnectInfo(
            "192.0.2.1:40000".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        api.router().oneshot(request).await.unwrap().status(),
        StatusCode::ACCEPTED
    );
}

mod stalled_memory;

#[tokio::test]
async fn early_health_reads_live_native_diagnostics_and_clears_detached_port() {
    use std::sync::atomic::AtomicU16;
    let mut api = EarlyApi::new(None);
    let port = Arc::new(AtomicU16::new(0));
    let attached = port.clone();
    let frontend = port.clone();
    api.frontend_identity = codeterm_api::health_routes::NativeFrontendIdentity {
        port: Arc::new(move || (frontend.load(Ordering::Acquire) != 0).then_some(14998)),
        identifier: "com.domios.dev.fixture".into(),
    };
    api.diagnostics_port =
        codeterm_api::health_routes::NativeDiagnosticsPort(Arc::new(move || {
            match attached.load(Ordering::Acquire) {
                0 => None,
                port => Some(port),
            }
        }));
    let router = api.router();
    for current in [0, 9398, 0] {
        port.store(current, Ordering::Release);
        let response = router
            .clone()
            .oneshot(request(Method::GET, "/api/health"))
            .await
            .unwrap();
        let expected = if current != 0 && cfg!(debug_assertions) {
            "9398"
        } else {
            ""
        };
        let body = json(response).await;
        assert_eq!(body["instance"]["cdp_port"], expected);
        assert_eq!(
            body["instance"]["vite_port"],
            if expected.is_empty() { "" } else { "14998" }
        );
        assert_eq!(
            body["instance"]["identifier"],
            if cfg!(debug_assertions) {
                "com.domios.dev.fixture"
            } else {
                ""
            }
        );
    }
}
