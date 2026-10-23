use super::{auth::CallerIdentity, routes::AppState};
use axum::{
    extract::{rejection::JsonRejection, Query, State},
    http::StatusCode,
    Extension, Json,
};
use codeterm_account::Status;
type Error = (StatusCode, String);
pub(crate) fn admit(caller: Option<&Extension<CallerIdentity>>) -> Result<(), Error> {
    if matches!(caller.map(|c| &c.0), Some(CallerIdentity::Local)) {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "Account access is local-only".into()))
    }
}
pub(crate) fn unavailable(message: String) -> Error {
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

pub(crate) const MAX_CALLBACK_REQUEST_BYTES: usize = 8 * 1024;
const MAX_CALLBACK_URL_BYTES: usize = 4 * 1024;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountCallback {
    url: String,
}

pub async fn callback(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    input: Result<Json<AccountCallback>, JsonRejection>,
) -> Result<StatusCode, Error> {
    dispatch_callback(caller.as_ref(), input, |callback| async move {
        let account = state
            .svc
            .account()
            .await
            .map_err(|_| unavailable("Account callback service unavailable".into()))?;
        account.complete(callback).await.map_err(callback_failure)
    })
    .await
}

async fn dispatch_callback<F, Fut>(
    caller: Option<&Extension<CallerIdentity>>,
    input: Result<Json<AccountCallback>, JsonRejection>,
    complete: F,
) -> Result<StatusCode, Error>
where
    F: FnOnce(codeterm_account::Callback) -> Fut,
    Fut: std::future::Future<Output = Result<(), Error>>,
{
    admit(caller)?;
    let Json(input) =
        input.map_err(|error| (error.status(), "Invalid account callback request".into()))?;
    if input.url.len() > MAX_CALLBACK_URL_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "Account callback exceeds limit".into(),
        ));
    }
    let callback = codeterm_account::Callback::parse(&input.url).map_err(callback_failure)?;
    complete(callback).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn callback_failure(error: codeterm_account::Error) -> Error {
    let status = match error {
        codeterm_account::Error::InvalidCallback => StatusCode::BAD_REQUEST,
        codeterm_account::Error::WrongState
        | codeterm_account::Error::Consumed
        | codeterm_account::Error::Cancelled
        | codeterm_account::Error::Timeout => StatusCode::CONFLICT,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    (status, "Account callback could not be accepted".into())
}

pub async fn logout(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<Status>, Error> {
    admit(caller.as_ref())?;
    let result = state.svc.account_logout().await;
    if let Ok(billing) = state.svc.billing().await {
        tokio::task::spawn_blocking(move || billing.snapshot(None))
            .await
            .map_err(|error| unavailable(error.to_string()))?;
    }
    Ok(Json(result.map_err(unavailable)?))
}
/// The plan snapshot plus the limits a client pre-checks creation against.
#[derive(serde::Serialize)]
pub struct PlanView {
    #[serde(flatten)]
    snapshot: codeterm_billing::Snapshot,
    limits: codeterm_service_core::service::billing::LimitSummary,
}
fn plan_view(state: &AppState, snapshot: codeterm_billing::Snapshot) -> Json<PlanView> {
    Json(PlanView {
        snapshot,
        limits: state.svc.limit_gate().summary(),
    })
}
pub async fn plan(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<PlanView>, Error> {
    admit(caller.as_ref())?;
    let snapshot = state.svc.account_plan().await.map_err(unavailable)?;
    Ok(plan_view(&state, snapshot))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upgrade {
    pub plan: String,
    #[serde(default)]
    pub theme: Option<String>,
}
impl Upgrade {
    pub fn checkout_theme(&self) -> Option<codeterm_billing::CheckoutTheme> {
        self.theme
            .as_deref()
            .and_then(codeterm_billing::CheckoutTheme::parse)
    }
}
pub async fn upgrade(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(input): Json<Upgrade>,
) -> Result<Json<serde_json::Value>, Error> {
    admit(caller.as_ref())?;
    state
        .svc
        .billing_open(Some(&input.plan), input.checkout_theme())
        .await
        .map_err(|message| match message.as_str() {
            codeterm_billing::ALREADY_SUBSCRIBED => (StatusCode::CONFLICT, message),
            _ => unavailable(message),
        })?;
    Ok(Json(serde_json::json!({"opened": true})))
}
pub async fn portal(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<serde_json::Value>, Error> {
    admit(caller.as_ref())?;
    state
        .svc
        .billing_open(None, None)
        .await
        .map_err(unavailable)?;
    Ok(Json(serde_json::json!({"opened": true})))
}
pub async fn refresh_plan(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<PlanView>, Error> {
    admit(caller.as_ref())?;
    let snapshot = state
        .svc
        .refresh_billing(codeterm_billing::RefreshEvent::Focus)
        .await
        .map_err(unavailable)?;
    Ok(plan_view(&state, snapshot))
}
#[cfg(test)]
mod tests;
