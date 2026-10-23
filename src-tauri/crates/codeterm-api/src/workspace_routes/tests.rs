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
