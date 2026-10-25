use super::*;
use codeterm_entitlements::{compiled_free, LimitGate};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;

#[tokio::test]
async fn locked_metadata_routes_preserve_typed_forbidden() {
    let (mut mux, _) = MultiplexerState::new();
    mux.set_limit_gate(codeterm_entitlements::LimitGate::dormant());
    let workspace = ["one", "two", "three"]
        .map(|name| mux.create_workspace(name.into(), None).id)
        .last()
        .cloned()
        .unwrap();
    let tab = mux.create_tab(&workspace).unwrap();
    mux.set_limit_gate(LimitGate::new(compiled_free(), true));
    let fixture = ServiceFixture::new(mux);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        "Bearer preview-test-token".parse().unwrap(),
    );
    let rename = rename_tab(
        AxumState(state.clone()),
        headers.clone(),
        Path(tab.clone()),
        Json(RenameTabBody {
            title: "Renamed".into(),
        }),
    )
    .await
    .unwrap_err();
    let describe = describe_tab(
        AxumState(state),
        headers,
        Path(tab),
        Json(DescribeTabBody {
            description: "Description".into(),
        }),
    )
    .await
    .unwrap_err();
    for response in [rename, describe] {
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "plan_limit_locked");
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("locked"));
    }
}

#[test]
fn metadata_errors_keep_not_found_validation_and_internal_statuses() {
    for (error, status) in [
        ("Tab not found: missing", StatusCode::NOT_FOUND),
        ("description must be one line", StatusCode::BAD_REQUEST),
        (
            "task-bound rename refused: retained identity",
            StatusCode::FORBIDDEN,
        ),
        ("storage write failed", StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        assert_eq!(map_metadata_error(error.into()).status(), status);
    }
}
