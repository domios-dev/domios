use super::{auth::CallerIdentity, routes::AppState};
use axum::{extract::State, http::StatusCode, Extension, Json};
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
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<Status>, Error> {
    admit(caller.as_ref())?;
    Ok(Json(state.svc.account_login().await.map_err(unavailable)?))
}
pub async fn logout(
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
            .logout()
            .await
            .map_err(|e| unavailable(e.to_string()))?,
    ))
}
#[cfg(test)]
mod tests;
