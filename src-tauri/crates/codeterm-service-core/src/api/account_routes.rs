use super::{auth::CallerIdentity, routes::AppState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};
use codeterm_account::Status;
type Error = (StatusCode, String);
fn admit(caller: Option<&Extension<CallerIdentity>>) -> Result<(), Error> {
    if matches!(caller.map(|c| &c.0), Some(CallerIdentity::Local)) {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "Account access is local-only".into()))
    }
}
fn unavailable(message: String) -> Error {
    (StatusCode::SERVICE_UNAVAILABLE, message)
}
pub async fn status(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<Status>, Error> {
    admit(caller.as_ref())?;
    Ok(Json(
        state
            .svc
            .account()
            .await
            .map_err(unavailable)?
            .status()
            .await,
    ))
}
pub async fn login(
    State(state): State<AppState>,
    Query(hint): Query<codeterm_account::LoginHint>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<Status>, Error> {
    admit(caller.as_ref())?;
    Ok(Json(
        state.svc.account_login(hint).await.map_err(unavailable)?,
    ))
}
pub async fn logout(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<Status>, Error> {
    admit(caller.as_ref())?;
    let account = state.svc.account().await.map_err(unavailable)?;
    let result = account.logout().await;
    if let Ok(billing) = state.svc.billing().await {
        billing.snapshot(None);
    }
    Ok(Json(result.map_err(|e| unavailable(e.to_string()))?))
}
pub async fn plan(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<codeterm_billing::Snapshot>, Error> {
    admit(caller.as_ref())?;
    Ok(Json(state.svc.account_plan().await.map_err(unavailable)?))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upgrade {
    pub plan: String,
}
pub async fn upgrade(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(input): Json<Upgrade>,
) -> Result<Json<serde_json::Value>, Error> {
    admit(caller.as_ref())?;
    state
        .svc
        .billing_open(Some(&input.plan))
        .await
        .map_err(unavailable)?;
    Ok(Json(serde_json::json!({"opened": true})))
}
pub async fn portal(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<serde_json::Value>, Error> {
    admit(caller.as_ref())?;
    state.svc.billing_open(None).await.map_err(unavailable)?;
    Ok(Json(serde_json::json!({"opened": true})))
}
pub async fn refresh_plan(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<codeterm_billing::Snapshot>, Error> {
    admit(caller.as_ref())?;
    Ok(Json(
        state
            .svc
            .refresh_billing(codeterm_billing::RefreshEvent::Focus)
            .await
            .map_err(unavailable)?,
    ))
}
#[cfg(test)]
mod tests;
