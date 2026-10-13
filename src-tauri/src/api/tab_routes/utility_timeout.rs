use super::*;

#[derive(Deserialize)]
pub struct TimeoutBody {
    timeout: String,
}

pub async fn set_timeout(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<TimeoutBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, error)| (status, error.to_string()))?;
    let tab_id =
        resolve_tab(&state, &raw_tab_id).map_err(|(status, error)| (status, error.to_string()))?;
    let owner = caller_tab_header(&headers).ok_or((
        StatusCode::BAD_REQUEST,
        "timeout requires an owning agent".into(),
    ))?;
    let service = state.svc.clone();
    tokio::task::spawn_blocking(move || {
        service.set_utility_timeout(&tab_id, &owner, &body.timeout)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(Json(serde_json::json!({"ok": true})))
}
