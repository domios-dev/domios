use crate::{auth::CallerIdentity, plugin_run_view_routes::DocumentLease, routes::AppState};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    plugin_id: String,
    operation: String,
    decision_key: String,
    action: Action,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Preview,
    Approve,
    CheckPrice,
    Check,
    Cancel,
}
impl Action {
    fn name(&self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Approve => "approve",
            Self::CheckPrice => "check_price",
            Self::Check => "check",
            Self::Cancel => "cancel",
        }
    }
}

pub async fn decide(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    if !matches!(
        caller.as_ref().map(|caller| &caller.0),
        Some(CallerIdentity::Local)
    ) {
        return Err((StatusCode::FORBIDDEN, "local owner required"));
    }
    tokio::task::spawn_blocking(move || {
        let Ok(document) = DocumentLease::capture(&state.svc, &request.plugin_id, &headers) else {
            return Json(json!({"ok":false,"code":"denied","reason":"stale"}));
        };
        if matches!(request.action, Action::Preview) {
            return Json(
                match codeterm_sentinel_host::preview_plugin_operation(
                    &state.svc,
                    &document.context,
                    &request.operation,
                    &request.decision_key,
                ) {
                    Ok(preview) => json!({"ok":true,"preview":preview}),
                    Err(_) => json!({"ok":false,"code":"denied","reason":"operation"}),
                },
            );
        }
        match codeterm_sentinel_host::decide_plugin_operation(
            &state.svc,
            &document.context,
            &request.operation,
            &request.decision_key,
            request.action.name(),
        ) {
            Ok(result) => Json(json!({"ok":true,"result":result})),
            Err(_) => Json(json!({"ok":false,"code":"denied","reason":"operation"})),
        }
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "operation unavailable"))
}
