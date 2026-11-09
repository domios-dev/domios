use axum::{body::Body, http::Request, http::StatusCode};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use tower::ServiceExt;

const FOCUS_EVENTS: [&str; 2] = ["tab-focused", "tab-focus-requested"];

fn fixture() -> (ServiceFixture, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("other".into(), None).id;
    let tab = mux.create_tab(&workspace).unwrap();
    (ServiceFixture::new(mux), workspace, tab)
}

async fn post(router: &axum::Router, path: &str) -> StatusCode {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

fn recorded_focus_events(fixture: &ServiceFixture) -> Vec<String> {
    fixture
        .ui
        .0
        .lock()
        .iter()
        .map(|(event, _)| event.clone())
        .filter(|event| FOCUS_EVENTS.contains(&event.as_str()))
        .collect()
}

#[tokio::test]
async fn a_client_selection_route_records_the_selection_without_a_focus_event() {
    let (fixture, workspace, tab) = fixture();
    let router = crate::routes::create_router(crate::transport_tabs::fixture::app_state(&fixture));

    let status = post(
        &router,
        &format!("/api/workspaces/{workspace}/tabs/{tab}/select"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let mux = fixture.service.mux.read();
    assert_eq!(
        mux.get_active_workspace().map(|w| w.id.clone()),
        Some(workspace)
    );
    assert_eq!(mux.get_focused_tab_id(), Some(tab.as_str()));
    drop(mux);
    assert!(recorded_focus_events(&fixture).is_empty());
}

#[tokio::test]
async fn a_tab_selection_route_refuses_an_unknown_tab() {
    let (fixture, _, _) = fixture();
    let router = crate::routes::create_router(crate::transport_tabs::fixture::app_state(&fixture));

    assert_eq!(
        post(&router, "/api/tabs/missing/select").await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_explicit_focus_route_asks_every_client_to_show_the_tab() {
    let (fixture, workspace, tab) = fixture();
    let router = crate::routes::create_router(crate::transport_tabs::fixture::app_state(&fixture));

    assert_eq!(
        post(&router, &format!("/api/tabs/{tab}/focus")).await,
        StatusCode::OK
    );

    let recorded = fixture.ui.0.lock().clone();
    let request = recorded
        .iter()
        .find(|(event, _)| event == "tab-focus-requested")
        .map(|(_, payload)| payload.clone())
        .expect("explicit focus emits its request");
    assert_eq!(request["tab_id"], tab.as_str());
    assert_eq!(request["workspace_id"], workspace.as_str());
}
