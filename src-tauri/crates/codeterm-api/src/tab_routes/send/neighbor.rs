use super::*;

pub(super) async fn authorize_message(
    state: &AppState,
    headers: &HeaderMap,
    target: &str,
    body: &SendTextBody,
    service_push: bool,
) -> Result<(), SendRejection> {
    if service_push || crate::auth::caller_peer_id(state, headers).is_some() {
        return Ok(());
    }
    let Some(caller) = crate::auth::tab_caller(state, headers)
        .map_err(|_| SendRejection::from((StatusCode::FORBIDDEN, "Invalid caller credential")))?
    else {
        return Ok(());
    };
    state.svc.authorize_neighbor_tab_message(&caller, target, body.text.is_some() && !body.raw && body.actions.is_none() && body.batch.is_none()).await.map_err(|message| SendRejection {
        status: StatusCode::FORBIDDEN,
        body: serde_json::json!({"ok":false,"error":{"code":message.split(':').next().unwrap_or("neighbor_capability_denied"),"message":message,"bytes_written":0}}),
    })
}
