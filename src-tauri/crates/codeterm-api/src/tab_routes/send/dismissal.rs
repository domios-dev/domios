use super::*;
use codeterm_service_core::service::pending_delivery_payloads::user_sends::dismissal::UserSendDismissal;

pub async fn chat_send_dismissals(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<UserSendDismissal>>, SendRejection> {
    check_auth(&state, &headers).map_err(SendRejection::from)?;
    let tab = resolve_tab(&state, &id).map_err(SendRejection::from)?;
    let svc = state.svc.clone();
    let receipts = tokio::task::spawn_blocking(move || {
        svc.pending_delivery_payloads.user_sends.dismissals(&tab)
    })
    .await
    .map_err(|_| SendRejection::from((StatusCode::INTERNAL_SERVER_ERROR, "dismissal read failed")))?
    .map_err(|_| {
        SendRejection::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "dismissal read unavailable",
        ))
    })?;
    Ok(Json(receipts))
}

pub async fn chat_send_delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UserSendDismissal>,
) -> Result<Json<UserSendDismissal>, SendRejection> {
    check_auth(&state, &headers).map_err(SendRejection::from)?;
    let tab = resolve_tab(&state, &id).map_err(SendRejection::from)?;
    let svc = state.svc.clone();
    let receipt = tokio::task::spawn_blocking(move || svc.delete_chat_send(&tab, body))
        .await
        .map_err(|_| {
            SendRejection::from((StatusCode::INTERNAL_SERVER_ERROR, "dismissal task failed"))
        })?
        .map_err(|detail| chat_admission_rejection(&id, detail))?;
    Ok(Json(receipt))
}
