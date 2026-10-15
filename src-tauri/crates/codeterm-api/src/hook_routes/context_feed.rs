use crate::routes::{check_auth, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::service::context_feed::{apply, update};
use serde_json::{json, Value};

pub(crate) async fn ingest(
    State(state): State<AppState>,
    Path(tab_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let provider = state
        .svc
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.agent_type.clone());
    let context = provider.as_deref().and_then(|provider| {
        state
            .svc
            .agent_registry
            .with_provider(provider, |provider| provider.parse_context_usage(&body))
            .flatten()
    });
    let changed = if body
        .pointer("/context_window/current_usage")
        .is_some_and(Value::is_null)
    {
        update(
            &state.svc,
            &tab_id,
            body.get("session_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            super::header_str(&headers, "x-codeterm-binding-nonce").unwrap_or_default(),
            None,
        )
    } else {
        context.is_some_and(|context| {
            apply(
                &state.svc,
                &tab_id,
                body.get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                super::header_str(&headers, "x-codeterm-binding-nonce").unwrap_or_default(),
                context,
            )
        })
    };
    Ok(Json(json!({"ok":true,"changed":changed})))
}
