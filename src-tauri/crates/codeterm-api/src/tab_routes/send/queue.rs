use super::*;
use codeterm_service_core::service::pending_delivery_payloads::user_sends::queue::{
    ComposerSendQueue, QueuedSendAction, QueuedSendOutcome,
};

pub async fn chat_send_queue(
    AxumState(state): AxumState<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ComposerSendQueue>, SendRejection> {
    let tab = crate::routes::resolve_tab_async(&state, &id)
        .await
        .map_err(SendRejection::from)?;
    let queue = tokio::task::spawn_blocking(move || state.svc.composer_send_queue(&tab))
        .await
        .map_err(|_| SendRejection::from((StatusCode::INTERNAL_SERVER_ERROR, "Queue read failed")))?
        .map_err(|detail| chat_admission_rejection(&id, detail))?;
    Ok(Json(queue))
}

pub async fn chat_send_queue_change(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(action): Json<QueuedSendAction>,
) -> Result<Json<QueuedSendOutcome>, SendRejection> {
    if crate::auth::remote_agent(&state, &headers).map_err(|(status, message)| SendRejection {
        status,
        body: serde_json::json!({"ok": false, "error": {"code": "forbidden", "message": message}}),
    })?.is_some() {
        return Err(SendRejection::from((StatusCode::FORBIDDEN, "Remote agents cannot edit composer sends")));
    }
    let tab = crate::routes::resolve_tab_async(&state, &id)
        .await
        .map_err(SendRejection::from)?;
    let outcome = tokio::task::spawn_blocking(move || {
        chat_send_allowed(&state, &tab)?;
        state.svc.change_composer_send_queue(&tab, action).map_err(|message| SendRejection {
            status: StatusCode::CONFLICT,
            body: serde_json::json!({"ok": false, "error": {"code": "queue_change_unavailable", "message": message}}),
        })
    })
    .await
    .map_err(|_| SendRejection::from((StatusCode::INTERNAL_SERVER_ERROR, "Queue edit failed")))??;
    Ok(Json(outcome))
}
