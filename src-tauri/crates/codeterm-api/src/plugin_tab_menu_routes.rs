//! REST surface for manifest tab menu contributions and host-owned receiver targets.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::routes::AppState;
use codeterm_service_core::service::ReceiverTargetError;
use codeterm_service_core::service::{ServiceContext, ServiceState};

type Refusal = (StatusCode, Json<Value>);
type RouteResult = Result<Json<Value>, Refusal>;

fn refusal(status: StatusCode, code: &str, message: &str) -> Refusal {
    (
        status,
        Json(json!({ "ok": false, "error": { "code": code, "message": message } })),
    )
}

fn target_refusal(error: ReceiverTargetError) -> Refusal {
    let status = match error {
        ReceiverTargetError::TabNotAgent => StatusCode::BAD_REQUEST,
        ReceiverTargetError::PersistFailed => StatusCode::INTERNAL_SERVER_ERROR,
        ReceiverTargetError::ReceiverNotFound
        | ReceiverTargetError::TabNotFound
        | ReceiverTargetError::MenuItemNotFound => StatusCode::NOT_FOUND,
    };
    refusal(status, error.code(), error.message())
}

fn internal() -> Refusal {
    refusal(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "Receiver target request did not complete",
    )
}

async fn on_blocking_pool<T: Serialize + Send + 'static>(
    svc: ServiceState,
    work: impl FnOnce(&ServiceContext) -> Result<T, ReceiverTargetError> + Send + 'static,
) -> RouteResult {
    let result = tokio::task::spawn_blocking(move || work(svc.as_ref()))
        .await
        .map_err(|_| internal())?
        .map_err(target_refusal)?;
    serde_json::to_value(result)
        .map(Json)
        .map_err(|_| internal())
}

pub async fn tab_menu(AxumState(state): AxumState<AppState>, _headers: HeaderMap) -> RouteResult {
    on_blocking_pool(state.svc.clone(), |svc| Ok(svc.tab_menu_state())).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TabMenuActionRequest {
    plugin_id: String,
    item_id: String,
    tab_id: String,
}

pub async fn tab_menu_action(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(request): Json<TabMenuActionRequest>,
) -> RouteResult {
    on_blocking_pool(state.svc.clone(), move |svc| {
        svc.run_tab_menu_item(&request.plugin_id, &request.item_id, &request.tab_id)
    })
    .await
}

pub async fn receiver_target(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(plugin_id): Path<String>,
) -> RouteResult {
    on_blocking_pool(state.svc.clone(), move |svc| {
        svc.receiver_target(&plugin_id)
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetReceiverTargetRequest {
    tab_id: String,
}

pub async fn set_receiver_target(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(plugin_id): Path<String>,
    Json(request): Json<SetReceiverTargetRequest>,
) -> RouteResult {
    on_blocking_pool(state.svc.clone(), move |svc| {
        svc.set_receiver_target(&plugin_id, &request.tab_id)
    })
    .await
}

pub async fn clear_receiver_target(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(plugin_id): Path<String>,
) -> RouteResult {
    on_blocking_pool(state.svc.clone(), move |svc| {
        svc.clear_receiver_target(&plugin_id)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pebble_receiver_target_route_refusals_map_codes_to_statuses() {
        for (error, status) in [
            (ReceiverTargetError::ReceiverNotFound, StatusCode::NOT_FOUND),
            (ReceiverTargetError::TabNotFound, StatusCode::NOT_FOUND),
            (ReceiverTargetError::TabNotAgent, StatusCode::BAD_REQUEST),
            (ReceiverTargetError::MenuItemNotFound, StatusCode::NOT_FOUND),
            (
                ReceiverTargetError::PersistFailed,
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ] {
            let (actual, Json(body)) = target_refusal(error);
            assert_eq!(actual, status);
            assert_eq!(body["ok"], false);
            assert_eq!(body["error"]["code"], error.code());
        }
    }
}
