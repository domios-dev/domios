use crate::api::routes::create_router;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[path = "../../transport_tabs/fixture.rs"]
mod fixture;

#[test]
fn transport_domain_registration_contract_routers_merge_without_collisions() {
    let _: axum::Router<crate::api::routes::AppState> =
        super::super::domain_transport_routes(axum::Router::new());
}

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
async fn transport_domain_registration_contract_auth_methods_and_persisted_cwd() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let app = create_router(fixture::app_state(&fixture));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65536).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["apiVersion"],
        2
    );
    let routes = [
        ("POST", format!("/api/tabs/{tab}/cwd")),
        ("POST", "/api/mux/restore-orphaned".into()),
        ("GET", format!("/api/tabs/{tab}/history")),
        ("GET", "/api/agents/providers/unknown/model-usage".into()),
        ("GET", "/api/system/default-cwd".into()),
        ("GET", "/api/system/shell-diagnostics".into()),
        ("POST", "/api/system/install-pwsh".into()),
        ("GET", "/api/themes/location".into()),
        ("POST", "/api/attachments/path-kinds".into()),
        ("POST", "/api/diagnostics/webgl".into()),
    ];
    for (method, path) in routes {
        for token in [None, Some("Bearer wrong")] {
            let mut request = Request::builder().method(method).uri(&path);
            if let Some(token) = token {
                request = request.header("authorization", token);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(&path)
                    .header("authorization", "Bearer preview-test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
    }
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/tabs/{tab}/cwd"))
        .header("authorization", "Bearer preview-test-token")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"cwd":"/transport-contract"}"#))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .cwd
            .as_deref(),
        Some("/transport-contract")
    );
    let request = Request::builder()
        .uri("/api/agents/providers/unknown/model-usage")
        .header("authorization", "Bearer preview-test-token")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!([])
    );
}

#[test]
fn transport_registry_contract_orphan_restore_body_is_explicit() {
    assert!(serde_json::from_value::<super::super::RestoreOrphanedTab>(
        serde_json::json!({"tab_id": "orphan"})
    )
    .is_ok());
    for body in [
        serde_json::json!({}),
        serde_json::json!({"tab_id": "orphan", "command": "arbitrary"}),
    ] {
        assert!(serde_json::from_value::<super::super::RestoreOrphanedTab>(body).is_err());
    }
}

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
async fn transport_registry_contract_owner_mutations_restore_and_persist() {
    let (mut mux, tab) = MultiplexerState::new();
    let workspace = mux.workspaces[0].id.clone();
    mux.rename_tab(&tab, "Restored by HTTP").unwrap();
    mux.close_workspace(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let state = fixture::app_state(&fixture);
    let job = state
        .cron
        .lock()
        .create(
            serde_json::from_value(serde_json::json!({
                "name": "Before", "workspace_id": workspace, "schedule": "0 0 * * *",
                "task": "opaque-task", "enabled": false
            }))
            .unwrap(),
        )
        .unwrap();
    for owner in [false, true] {
        let mut headers = axum::http::HeaderMap::new();
        if owner {
            headers.insert(
                "authorization",
                "Bearer preview-test-token".parse().unwrap(),
            );
        }
        let update = super::super::owner_cron_update(
            axum::extract::State(state.clone()),
            headers.clone(),
            axum::extract::Path(job.id.clone()),
            axum::Json(serde_json::from_value(serde_json::json!({"name":"After"})).unwrap()),
        )
        .await;
        if owner {
            update.unwrap();
            assert_eq!(state.cron.lock().get(&job.id).unwrap().name, "After");
        } else {
            assert_eq!(update.unwrap_err().0, StatusCode::FORBIDDEN);
            assert_eq!(state.cron.lock().get(&job.id).unwrap().name, "Before");
        }
        let restored = super::super::restore_orphaned_tab(
            axum::extract::State(state.clone()),
            headers,
            axum::Json(super::super::RestoreOrphanedTab {
                tab_id: tab.clone(),
            }),
        )
        .await;
        if owner {
            let body = restored.unwrap().0;
            let restored_tab = body["tab_id"].as_str().unwrap();
            assert_eq!(
                fixture
                    .service
                    .mux
                    .read()
                    .get_tab(restored_tab)
                    .unwrap()
                    .title,
                "Restored by HTTP"
            );
        } else {
            assert_eq!(restored.unwrap_err().0, StatusCode::FORBIDDEN);
            assert!(fixture.service.mux.read().get_tab(&tab).is_none());
        }
    }
}
