use super::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;

#[tokio::test]
async fn reorder_routes_preserve_mux_error_code_and_offending_id() {
    use axum::response::IntoResponse;

    let (mut mux, parent) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&parent).unwrap();
    let child = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&child).unwrap().parent_tab_id = Some(parent.clone());
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let foreign = "mesh:peer/worker";
    let body = || ReorderTabsBody {
        ordered_tab_ids: vec![foreign.into(), child.clone()],
    };
    let errors = [
        reorder_tabs(
            AxumState(state.clone()),
            HeaderMap::new(),
            Path(workspace.clone()),
            Json(body()),
        )
        .await
        .unwrap_err(),
        reorder_children(
            AxumState(state.clone()),
            HeaderMap::new(),
            Path(parent),
            Json(body()),
        )
        .await
        .unwrap_err(),
        reorder_display(
            AxumState(state),
            HeaderMap::new(),
            Path(workspace),
            Json(body()),
        )
        .await
        .unwrap_err(),
    ];
    for error in errors {
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], "reorder_failed");
        assert_eq!(
            value["error"]["message"],
            format!("Tab not in workspace: {foreign}")
        );
    }
}

#[tokio::test]
async fn reopen_tab_and_group_routes_preserve_typed_plan_refusals() {
    let (mut mux, first) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&first).unwrap();
    let closed = mux.create_tab(&workspace).unwrap();
    mux.close_tab(&closed).unwrap();
    let group = mux.snapshot().recently_closed[0].group_id.clone();
    for _ in 0..34 {
        mux.create_tab(&workspace).unwrap();
    }
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let single = reopen_closed_tab(
        AxumState(state.clone()),
        HeaderMap::new(),
        Path(workspace),
        OptionalJson(None),
    )
    .await
    .unwrap_err();
    let group = reopen_closed_group(AxumState(state), HeaderMap::new(), Path(group))
        .await
        .unwrap_err();
    for (status, Json(body)) in [single, group] {
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "free_limit_reached");
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("free_limit_reached: "));
    }
    assert_eq!(fixture.service.mux.read().all_tabs().count(), 35);
}

#[tokio::test]
async fn reopen_tab_and_group_routes_preserve_locked_workspace_refusals() {
    let mut mux = MultiplexerState::empty();
    for _ in 0..3 {
        mux.create_workspace("Other".into(), None);
    }
    let locked_workspace = mux.locks().workspaces.iter().next().unwrap().to_owned();
    let closed = mux.create_tab(&locked_workspace).unwrap();
    mux.close_tab(&closed).unwrap();
    let locked_group = mux.snapshot().recently_closed[0].group_id.clone();
    let before = serde_json::to_value(mux.snapshot()).unwrap();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let single = reopen_closed_tab(
        AxumState(state.clone()),
        HeaderMap::new(),
        Path(locked_workspace),
        OptionalJson(None),
    )
    .await
    .unwrap_err();
    let group = reopen_closed_group(AxumState(state), HeaderMap::new(), Path(locked_group))
        .await
        .unwrap_err();
    for (status, Json(body)) in [single, group] {
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "plan_limit_locked");
    }
    assert_eq!(
        serde_json::to_value(fixture.service.mux.read().snapshot()).unwrap(),
        before
    );
}
