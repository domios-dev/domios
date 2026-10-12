use codeterm_frontend_bundle::{activation::Activation, manifest, store::Store};

const METADATA: &[u8] =
    include_bytes!("../../../../scripts/frontend-release/fixtures/frontend-manifest.json");
const SIGNATURE: &str =
    include_str!("../../../../scripts/frontend-release/fixtures/frontend-manifest.json.minisig");
const ARCHIVE: &[u8] =
    include_bytes!("../../../../scripts/frontend-release/fixtures/frontend-v1.0.0.tar.gz");
const PUBLIC_KEY: &str = "RWRmcm9udGVuZNEdQEUZEh9CtfmYUwGDKXT9+qbRXfdx90/9a+hv/8OA";

fn verified() -> manifest::VerifiedManifest {
    manifest::verify(METADATA, SIGNATURE, ARCHIVE, PUBLIC_KEY, 2, false).unwrap()
}
#[tokio::test]
async fn frontend_bundle_routes_contract_real_handlers_and_authority() {
    use crate::api::frontend_bundle_routes::{routes, FrontendRouteState};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    let bundles = Arc::new(
        codeterm_frontend_bundle::FrontendBundles::open(
            root.path(),
            2,
            "0.0.0",
            None,
            "https://example.test/manifest.json".into(),
            false,
            Arc::new(|_, _| {}),
        )
        .unwrap(),
    );
    let app = routes(FrontendRouteState {
        bundles,
        is_owner: Arc::new(|headers| {
            headers
                .get("authorization")
                .is_some_and(|value| value == "Bearer owner")
        }),
    });
    let body = serde_json::json!({"revision": 1, "payload": {"drafts": {"t": {"text": "last edit", "attachmentIds": ["a"]}}, "scroll": {"t": "anchor"}}}).to_string();
    for authority in [None, Some("Bearer peer"), Some("Bearer push")] {
        let mut request = Request::builder()
            .method("PUT")
            .uri("/api/frontend/clients/client-a/checkpoint")
            .header("content-type", "application/json");
        if let Some(authority) = authority {
            request = request.header("authorization", authority);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.clone())).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/frontend/clients/client-a/checkpoint")
                .header("authorization", "Bearer owner")
                .header("content-type", "application/json")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let saved: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(
        saved,
        serde_json::from_str::<serde_json::Value>(&body).unwrap()
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/frontend/clients/client-a/checkpoint")
                .header("authorization", "Bearer owner")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let restored: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(restored, saved);
    for (path, body) in [
        ("/api/frontend/check", "{}"),
        (
            "/api/frontend/activate",
            r#"{"releaseId":"frontend-v1.0.0","clients":["client-a"]}"#,
        ),
        (
            "/api/frontend/healthy",
            r#"{"releaseId":"frontend-v1.0.0","generation":1,"clientId":"client-a","token":"foreign"}"#,
        ),
        (
            "/api/frontend/rollback",
            r#"{"releaseId":"frontend-v1.0.0","generation":1}"#,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", "Bearer owner")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT, "{path}");
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/frontend/status")
                .header("authorization", "Bearer owner")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(status["apiVersion"], 2);
    assert_eq!(status["updatesEnabled"], false);
    assert!(status.get("claims").is_none());
}

#[tokio::test]
async fn frontend_bundle_routes_contract_verified_activation_assets_health_and_rollback() {
    use crate::api::frontend_bundle_routes::{routes, FrontendRouteState};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    let mut store = Store::open(root.path()).unwrap();
    store.install(&verified(), SIGNATURE, ARCHIVE).unwrap();
    let mut state = Activation::default();
    state.stage(&verified()).unwrap();
    store.save(&state).unwrap();
    drop(store);
    let bundles = Arc::new(
        codeterm_frontend_bundle::FrontendBundles::open(
            root.path(),
            2,
            "0.0.0",
            Some(PUBLIC_KEY.into()),
            "https://example.test/manifest.json".into(),
            false,
            Arc::new(|_, _| {}),
        )
        .unwrap(),
    );
    let app = routes(FrontendRouteState {
        bundles,
        is_owner: Arc::new(|headers| {
            headers
                .get("authorization")
                .is_some_and(|value| value == "Bearer owner")
        }),
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/frontend/activate")
                .header("authorization", "Bearer owner")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"releaseId":"frontend-v1.0.0","clients":["client-a"]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let activation: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
    assert_eq!(activation["releaseId"], "frontend-v1.0.0");
    let healthy = serde_json::json!({"releaseId": activation["releaseId"], "generation": activation["generation"], "clientId": "client-a", "token": activation["claims"]["client-a"]});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/frontend/healthy")
                .header("authorization", "Bearer owner")
                .header("content-type", "application/json")
                .body(Body::from(healthy.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
    assert_eq!(status["active"]["releaseId"], "frontend-v1.0.0");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/frontend/frontend-v1.0.0/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    let html = String::from_utf8(
        to_bytes(response.into_body(), 16384)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.find("/frontend/bootstrap.js").unwrap() < html.find("assets/fixture.js").unwrap());
    let response = app.clone().oneshot(Request::builder().method("POST").uri("/api/frontend/rollback").header("authorization", "Bearer owner").header("content-type", "application/json").body(Body::from(serde_json::json!({"releaseId": activation["releaseId"], "generation": activation["generation"]}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
    assert!(status["active"].is_null());
    assert_eq!(status["quarantined"][0], "frontend-v1.0.0");
}

#[test]
fn release_contract_never_falls_behind_the_bundled_frontend() {
    // A frontend-only release advances the contract alone; an app release syncs both.
    let contract: serde_json::Value =
        serde_json::from_str(include_str!("../../../../frontend-release-contract.json")).unwrap();
    let contract = semver::Version::parse(contract["version"].as_str().unwrap()).unwrap();
    let bundled =
        semver::Version::parse(crate::frontend_bundle_api_version::BUNDLED_FRONTEND_VERSION)
            .unwrap();
    assert!(
        contract >= bundled,
        "contract {contract} is behind bundled {bundled}"
    );
}
