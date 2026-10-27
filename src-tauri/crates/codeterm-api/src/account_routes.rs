use super::{auth::CallerIdentity, routes::AppState};
use axum::{
    extract::{rejection::JsonRejection, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
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
pub(crate) fn debug_login_router() -> axum::Router<AppState> {
    let router = axum::Router::new();
    #[cfg(debug_assertions)]
    let router = router.route(
        "/api/account/login/pending",
        axum::routing::get(pending_login),
    );
    router
}

#[cfg(debug_assertions)]
pub async fn pending_login(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<(axum::http::HeaderMap, Json<serde_json::Value>), Error> {
    admit(caller.as_ref())?;
    let url = state
        .svc
        .account()
        .await
        .map_err(unavailable)?
        .pending_login_url()
        .await
        .ok_or((StatusCode::NOT_FOUND, "No pending account login".into()))?;
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok((headers, Json(serde_json::json!({"url": url.as_str()}))))
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
fn portal_failure(message: String) -> axum::response::Response {
    use axum::response::IntoResponse;
    if message == codeterm_billing::NO_BILLING_CUSTOMER {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"code": "no_billing_customer", "message": message})),
        )
            .into_response();
    }
    unavailable(message).into_response()
}

pub async fn portal(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<serde_json::Value>, axum::response::Response> {
    use axum::response::IntoResponse;
    admit(caller.as_ref()).map_err(IntoResponse::into_response)?;
    state
        .svc
        .billing_open(None, None)
        .await
        .map_err(portal_failure)?;
    Ok(Json(serde_json::json!({"opened": true})))
}

pub async fn neighbors(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(command): Json<codeterm_service_core::service::neighbors::NeighborCommand>,
) -> Result<Json<serde_json::Value>, Error> {
    match caller.as_ref().map(|caller| &caller.0) {
        Some(CallerIdentity::Local) => {}
        Some(CallerIdentity::Tab(actor))
            if matches!(
                &command,
                codeterm_service_core::service::neighbors::NeighborCommand::List {}
            ) =>
        {
            state
                .svc
                .require_neighbor_general(Some(&actor.tab_id), Some(&actor.binding_nonce))
                .map_err(|message| (StatusCode::FORBIDDEN, message))?;
        }
        _ => return Err((StatusCode::FORBIDDEN, "Account access is local-only".into())),
    }
    state
        .svc
        .neighbor_registry(command)
        .await
        .map(Json)
        .map_err(unavailable)
}

pub async fn neighbor_runtime(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    _headers: axum::http::HeaderMap,
    Json(command): Json<codeterm_service_core::service::neighbors::NeighborRuntimeCommand>,
) -> Result<Json<serde_json::Value>, Response> {
    let (tab, nonce) = match caller.as_ref().map(|caller| &caller.0) {
        Some(CallerIdentity::Tab(caller)) => (
            Some(caller.tab_id.clone()),
            Some(caller.binding_nonce.as_str()),
        ),
        Some(CallerIdentity::Local) => (None, None),
        _ => return Err((StatusCode::FORBIDDEN, "Account access is local-only").into_response()),
    };
    state
        .svc
        .neighbor_runtime(command, tab.as_deref(), nonce)
        .await
        .map(Json)
        .map_err(neighbor_failure)
}

fn neighbor_failure(message: String) -> Response {
    for reason in ["rate_limited", "aggregate_limited"] {
        if let Some(detail) = message.strip_prefix(&format!("{reason}:")) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(detail) {
                if data["retry_after_seconds"].as_u64().is_some() {
                    return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({"code":reason,"reason":reason,"retry_after_seconds":data["retry_after_seconds"],"message":"Neighbor send limit reached"}))).into_response();
                }
            }
        }
    }
    if let Some(detail) = message.strip_prefix("neighbor_daily_limit:") {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(detail) {
            if data["limit"].as_u64().is_some() && data["resets_at"].as_u64().is_some() {
                return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({"code":"neighbor_daily_limit","reason":"neighbor_daily_limit","limit":data["limit"],"resets_at":data["resets_at"],"message":"Neighbor daily message limit reached"}))).into_response();
            }
        }
    }
    let reason = message.split_once(':').map(|(reason, _)| reason);
    let status = match reason {
        Some("neighbor_forbidden") => StatusCode::FORBIDDEN,
        Some("session_revoked") => StatusCode::UNAUTHORIZED,
        Some("neighbors_disabled") => StatusCode::SERVICE_UNAVAILABLE,
        Some("limit_out_of_range") => StatusCode::BAD_REQUEST,
        Some("invalid_request") => StatusCode::BAD_REQUEST,
        Some("friendship_not_found") => StatusCode::NOT_FOUND,
        Some("friends_locked" | "recipient_locked") => StatusCode::FORBIDDEN,
        Some(
            "receiver_elsewhere"
            | "friendship_ended"
            | "receiver_revision_stale"
            | "not_receiver"
            | "recipient_not_receiving"
            | "installation_unverified"
            | "operation_id_conflict",
        ) => StatusCode::CONFLICT,
        _ => return unavailable(message).into_response(),
    };
    (
        status,
        Json(serde_json::json!({"code":reason,"reason":reason,"message":message,"error":message})),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeighborEnabled {
    enabled: bool,
}

pub async fn neighbors_set_enabled(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(body): Json<NeighborEnabled>,
) -> Result<Json<serde_json::Value>, Error> {
    admit(caller.as_ref())?;
    let enabled = state
        .svc
        .set_neighbors_enabled(body.enabled)
        .await
        .map_err(unavailable)?;
    Ok(Json(serde_json::json!({"ok":true,"enabled":enabled})))
}

pub async fn billing_operation(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(operation): Json<codeterm_billing::PlanOperation>,
) -> Result<Json<serde_json::Value>, Error> {
    admit(caller.as_ref())?;
    state
        .svc
        .billing_plan_operation(operation)
        .await
        .map(Json)
        .map_err(unavailable)
}

pub async fn redeem_gift_key(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    request: Result<
        Json<codeterm_service_core::service::gift_redemption::GiftRedemption>,
        JsonRejection,
    >,
) -> Result<Json<serde_json::Value>, Error> {
    admit(caller.as_ref())?;
    let Json(request) = request.map_err(|_| (StatusCode::BAD_REQUEST, "invalid_request".into()))?;
    state
        .svc
        .redeem_gift_key(request)
        .await
        .map(Json)
        .map_err(unavailable)
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRefresh {
    #[serde(default)]
    focus: bool,
}

impl PlanRefresh {
    fn event(&self) -> codeterm_billing::RefreshEvent {
        if self.focus {
            codeterm_billing::RefreshEvent::Focus
        } else {
            codeterm_billing::RefreshEvent::Explicit
        }
    }
}

pub async fn refresh_plan(
    State(state): State<AppState>,
    Query(input): Query<PlanRefresh>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<PlanView>, Error> {
    admit(caller.as_ref())?;
    let snapshot = state
        .svc
        .refresh_billing(input.event())
        .await
        .map_err(unavailable)?;
    Ok(plan_view(&state, snapshot))
}
#[cfg(test)]
mod tests;
