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
            .analytics_worker()
            .await
            .map_err(unavailable)?
            .set_consent(body.consent)
            .await
            .map_err(unavailable)?,
    ))
}

pub async fn event(
    State(state): State<AppState>,
    headers: HeaderMap,
    caller: Option<Extension<CallerIdentity>>,
    Json(event): Json<Event>,
) -> Result<Json<serde_json::Value>, Error> {
    admit(&state, &headers, caller.as_ref())?;
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn analytics_routes_post_refuses_before_event_handler() {
        use axum::{body::Body, http::Request, routing::post, Router};
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        use tower::ServiceExt;
        let recorded = Arc::new(AtomicBool::new(false));
        let captured = recorded.clone();
        let router = Router::new().route(
            "/api/analytics/event",
            post(move |headers: HeaderMap| async move {
                authorize(
                    &headers,
                    Some("fixture-token"),
                    Some(&CallerIdentity::Local),
                )?;
                captured.store(true, Ordering::SeqCst);
                Ok::<_, Error>(StatusCode::OK)
            }),
        );
        let response = router
            .oneshot(
                Request::post("/api/analytics/event")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(!recorded.load(Ordering::SeqCst));
    }

    #[test]
    fn analytics_routes_unauthenticated_post_refused() {
        assert_eq!(
            authorize(
                &HeaderMap::new(),
                Some("fixture-token"),
                Some(&CallerIdentity::Local)
            )
            .unwrap_err()
            .0,
            StatusCode::UNAUTHORIZED
        );
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());
        assert_eq!(
            authorize(
                &headers,
                Some("fixture-token"),
                Some(&CallerIdentity::Local)
            )
            .unwrap_err()
            .0,
            StatusCode::UNAUTHORIZED
        );
        headers.insert("authorization", "Bearer fixture-token".parse().unwrap());
        assert!(authorize(
            &headers,
            Some("fixture-token"),
            Some(&CallerIdentity::Local)
        )
        .is_ok());
        assert_eq!(
            authorize(&headers, Some("fixture-token"), None)
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
    }
}
