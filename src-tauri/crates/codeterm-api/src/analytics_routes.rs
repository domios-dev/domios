use super::{auth::CallerIdentity, routes::AppState};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Extension, Json,
};
use codeterm_analytics::{Consent, Event, Status};
use serde::Deserialize;

type Error = (StatusCode, String);

fn authorize(
    headers: &HeaderMap,
    token: Option<&str>,
    caller: Option<&CallerIdentity>,
) -> Result<(), Error> {
    let supplied = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if token.is_none_or(str::is_empty) || supplied != token {
        return Err((StatusCode::UNAUTHORIZED, "Invalid or missing token".into()));
    }
    if !matches!(caller, Some(CallerIdentity::Local)) {
        return Err((
            StatusCode::FORBIDDEN,
            "Analytics settings and events are local-only".into(),
        ));
    }
    Ok(())
}

fn admit(
    state: &AppState,
    headers: &HeaderMap,
    caller: Option<&Extension<CallerIdentity>>,
) -> Result<(), Error> {
    authorize(
        headers,
        state.config.lock().api.token.as_deref(),
        caller.map(|caller| &caller.0),
    )
}

fn unavailable(message: String) -> Error {
    (StatusCode::SERVICE_UNAVAILABLE, message)
}

pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<Status>, Error> {
    admit(&state, &headers, caller.as_ref())?;
    Ok(Json(
        state
            .svc
            .analytics_worker()
            .await
            .map_err(unavailable)?
            .status()
            .await
            .map_err(unavailable)?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentBody {
    pub consent: Consent,
}

pub async fn consent(
    State(state): State<AppState>,
    headers: HeaderMap,
    caller: Option<Extension<CallerIdentity>>,
    Json(body): Json<ConsentBody>,
) -> Result<Json<Status>, Error> {
    admit(&state, &headers, caller.as_ref())?;
    Ok(Json(
        state
            .svc
            .set_product_analytics_consent(body.consent)
            .await
            .map_err(unavailable)?,
    ))
}

fn admit_client_event(event: &Event) -> Result<(), Error> {
    if matches!(
        event,
        Event::PlanViewed {}
            | Event::FeatureUsed { .. }
            | Event::LimitReachedShown { .. }
            | Event::UpgradeClicked { .. }
            | Event::FriendAdded { .. }
            | Event::NeighborMessageSent { .. }
            | Event::FileShared { .. }
            | Event::ReadReceiptShown { .. }
            | Event::SettingsViewed { .. }
    ) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Analytics event is daemon-owned".into(),
        ))
    }
}

pub async fn event(
    State(state): State<AppState>,
    headers: HeaderMap,
    caller: Option<Extension<CallerIdentity>>,
    Json(event): Json<Event>,
) -> Result<Json<serde_json::Value>, Error> {
    admit(&state, &headers, caller.as_ref())?;
    admit_client_event(&event)?;
    let queued = state
        .svc
        .analytics_worker()
        .await
        .map_err(unavailable)?
        .record(event)
        .await
        .map_err(unavailable)?;
    Ok(Json(serde_json::json!({"queued": queued})))
}

#[cfg(test)]
mod tests;
